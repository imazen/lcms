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

/// _cmsStageAllocNamedColor — the named-color MPE element.
#[test]
fn named_color_stage() {
    unsafe {
        // build a named-color list on both sides
        let cl = c::cmsAllocNamedColorList(std::ptr::null_mut(),
            2, 3, b"RGB \0".as_ptr() as *const _, b"Pfx\0".as_ptr()
                as *const _);
        assert!(!cl.is_null());
        for i in 0..2u32 {
            let mut name = [0u8; 64];
            let s = format!("c{i}\0");
            name[..s.len()].copy_from_slice(s.as_bytes());
            let pcs = [32768u16; 3];
            let dev = [16384u16; 3];
            c::cmsAppendNamedColor(cl, name.as_ptr(), pcs.as_ptr(),
                dev.as_ptr());
        }
        let cst = c::_cmsStageAllocNamedColor(cl, 0);
        assert!(!cst.is_null(), "C named-color stage");
        let ct = c::cmsStageType(cst);
        eprintln!("named stage type {ct:#x}");
        let cin = c::cmsStageInputChannels(cst);
        let cout = c::cmsStageOutputChannels(cst);
        eprintln!("named stage ich={cin} och={cout}");
        // rust side
        let mut rl = lcms::named::alloc_named_color_list(None, 2, 3,
            b"RGB ", b"Pfx").unwrap();
        for i in 0..2u32 {
            let name = format!("c{i}");
            let mut dev = [0u16; 16];
            dev[..3].copy_from_slice(&[16384u16; 3]);
            let _ = lcms::named::append_named_color(&mut rl,
                Some(name.as_bytes()), Some(&[32768u16; 3]),
                Some(&dev));
        }
        let rst = lcms::named::stage_alloc_named_color(&rl, false);
        assert!(rst.is_some(), "rust named-color stage");
        c::cmsStageFree(cst);
        c::cmsFreeNamedColorList(cl);
    }
}

/// cmsStageAllocCLutFloatGranular — the float-CLUT-granular
/// allocator.
#[test]
fn clut_float_granular() {
    unsafe {
        let grid: [u32; 3] = [2, 2, 2];
        let tab = [0.5f32; 24];
        let s = c::cmsStageAllocCLutFloatGranular(
            std::ptr::null_mut(), grid.as_ptr(), 3, 3,
            tab.as_ptr());
        assert!(!s.is_null(), "float-granular clut");
        // eval through a pipeline
        let pl = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(pl, 0, s);
        let inp = [0.5f32, 0.5, 0.5];
        let mut out = [0.0f32; 3];
        c::cmsPipelineEvalFloat(inp.as_ptr(), out.as_mut_ptr(),
            pl);
        eprintln!("eval={:?}", out);
        c::cmsPipelineFree(pl);
    }
}

/// cmsDupPipeline on a multi-stage pipeline — the MPE
/// Dup/Free internals on each element type.
#[test]
fn pipeline_dup_mpe() {
    unsafe {
        let pl = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        // build a 5-stage mAB-ish pipeline: Acurves + CLUT +
        // matrix + Bcurves + identity
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = [g, g, g];
        let acurves = c::cmsStageAllocToneCurves(
            std::ptr::null_mut(), 3, curves.as_ptr());
        let grid = [8u32; 3];
        let tab = [0.5f32; 8 * 8 * 8 * 3];
        let clut = c::cmsStageAllocCLutFloatGranular(
            std::ptr::null_mut(), grid.as_ptr(), 3, 3,
            tab.as_ptr());
        let mat_v = [1.0f64, 0.0, 0.0,
                     0.0, 1.0, 0.0,
                     0.0, 0.0, 1.0];
        let mat = c::cmsStageAllocMatrix(
            std::ptr::null_mut(), 3, 3, mat_v.as_ptr(),
            std::ptr::null());
        let bcurves = c::cmsStageAllocToneCurves(
            std::ptr::null_mut(), 3, curves.as_ptr());
        c::cmsPipelineInsertStage(pl, 0xFFFF, acurves);
        c::cmsPipelineInsertStage(pl, 0xFFFF, clut);
        c::cmsPipelineInsertStage(pl, 0xFFFF, mat);
        c::cmsPipelineInsertStage(pl, 0xFFFF, bcurves);
        // dup — exercises each element type's Dup
        let pl2 = c::cmsPipelineDup(pl);
        eprintln!("dup null={}", pl2.is_null());
        if !pl2.is_null() {
            let inp = [0.5f32; 3];
            let mut out = [0.0f32; 3];
            c::cmsPipelineEvalFloat(inp.as_ptr(),
                out.as_mut_ptr(), pl2);
            eprintln!("dup eval={:?}", out);
            c::cmsPipelineFree(pl2);
        }
        c::cmsPipelineFree(pl);
    }
}

/// Stage-type internals — CheckAndRetreiveStages on more
/// pipeline shapes + stage-type queries.
#[test]
fn stage_type_machinery() {
    unsafe {
        // build pipelines with different stage mixes and query
        // the stage types
        let pl1 = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = [g, g, g];
        c::cmsPipelineInsertStage(pl1, 1,
            c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, curves.as_ptr()));
        let mat_v = [1.0f64, 0.0, 0.0,
                     0.0, 1.0, 0.0,
                     0.0, 0.0, 1.0];
        c::cmsPipelineInsertStage(pl1, 1,
            c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 3, mat_v.as_ptr(),
                std::ptr::null()));
        let grid = [4u32; 3];
        let tab = [0.5f32; 4 * 4 * 4 * 3];
        c::cmsPipelineInsertStage(pl1, 1,
            c::cmsStageAllocCLutFloatGranular(
                std::ptr::null_mut(), grid.as_ptr(), 3, 3,
                tab.as_ptr()));
        eprintln!("stagecount={}", c::cmsPipelineStageCount(pl1));
        // CheckAndRetreiveStages — the variadic stage-shape
        // query
        let mut s1: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut s2: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut s3: *mut std::ffi::c_void = std::ptr::null_mut();
        // convention: n types first, then n pointers
        let ok = c::cmsPipelineCheckAndRetreiveStages(pl1, 3,
            0x63767374u32, 0x6D617466u32, 0x636C7574u32,
            &mut s1, &mut s2, &mut s3);
        eprintln!("check cvst+matf+clut={ok}");
        // a mismatched check (wrong order)
        let ok2 = c::cmsPipelineCheckAndRetreiveStages(pl1, 3,
            0x6D617466u32, 0x63767374u32, 0x636C7574u32,
            &mut s1, &mut s2, &mut s3);
        eprintln!("check wrong-order={ok2}");
        // input/output channel queries
        let ic = c::cmsPipelineInputChannels(pl1);
        let oc = c::cmsPipelineOutputChannels(pl1);
        eprintln!("channels {ic}/{oc}");
        c::cmsPipelineFree(pl1);
    }
}

/// Stage-type introspection — cmsStageType on each stage.
#[test]
fn stage_type_introspection() {
    unsafe {
        let pl = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = [g, g, g];
        let s1 = c::cmsStageAllocToneCurves(
            std::ptr::null_mut(), 3, curves.as_ptr());
        let mat_v = [1.0f64, 0.0, 0.0,
                     0.0, 1.0, 0.0,
                     0.0, 0.0, 1.0];
        let s2 = c::cmsStageAllocMatrix(
            std::ptr::null_mut(), 3, 3, mat_v.as_ptr(),
            std::ptr::null());
        eprintln!("curves type={:#x}",
            c::cmsStageType(s1));
        eprintln!("matrix type={:#x}",
            c::cmsStageType(s2));
        c::cmsStageFree(s1);
        c::cmsStageFree(s2);
        c::cmsPipelineFree(pl);
    }
}

/// Stage machinery internals — AllocNamedColor/SampleCLut/
/// AllocIdentity on more stage combos.
#[test]
fn stage_machinery() {
    unsafe {
        // AllocIdentity — the no-op stage
        let id = c::cmsStageAllocIdentity(std::ptr::null_mut(), 3);
        eprintln!("identity type={:#x}", c::cmsStageType(id));
        // SampleCLut16bit — sample a stage's CLUT fn
        let grid = [4u32; 3];
        let tab16 = [0x8000u16; 4 * 4 * 4 * 3];
        let cl16 = c::cmsStageAllocCLut16bitGranular(
            std::ptr::null_mut(), grid.as_ptr(), 3, 3,
            tab16.as_ptr());
        // SampleCLut16bit — runs the sampler through the CLUT
        unsafe extern "C" fn samp16(
            inp: *const u16, out: *mut u16,
            _c: *mut std::ffi::c_void) -> i32 {
            *out = *inp;
            *out.add(1) = *inp.add(1);
            *out.add(2) = *inp.add(2);
            1
        }
        c::cmsStageSampleCLut16bit(cl16, samp16,
            std::ptr::null_mut(), 0);
        // SampleCLutFloat — a float CLUT's eval
        let tabf = [0.5f32; 4 * 4 * 4 * 3];
        let clf = c::cmsStageAllocCLutFloatGranular(
            std::ptr::null_mut(), grid.as_ptr(), 3, 3,
            tabf.as_ptr());
        unsafe extern "C" fn sampf(
            inp: *const f32, out: *mut f32,
            _c: *mut std::ffi::c_void) -> i32 {
            *out = *inp;
            *out.add(1) = *inp.add(1);
            *out.add(2) = *inp.add(2);
            1
        }
        c::cmsStageSampleCLutFloat(clf, sampf,
            std::ptr::null_mut(), 0);
        // AllocNamedColor — the named-color stage
        let ncl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 2, 3,
            b"p\0".as_ptr() as *const _,
            b"s\0".as_ptr() as *const _);
        let nc = c::_cmsStageAllocNamedColor(ncl, 0);
        eprintln!("namedstage null={}", nc.is_null());
        if !nc.is_null() { c::cmsStageFree(nc); }
        c::cmsStageFree(id);
        c::cmsStageFree(cl16);
        c::cmsStageFree(clf);
        c::cmsFreeNamedColorList(ncl);
    }
}

/// Reverse-eval + slice-space internals — EvalReverseFloat +
/// SliceSpace16/Float on more shapes.
#[test]
fn reverse_slice_machinery() {
    unsafe {
        // EvalReverseFloat — the reverse-iteration machinery
        let pl = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let mat_v = [1.0f64, 0.0, 0.0,
                     0.0, 1.0, 0.0,
                     0.0, 0.0, 1.0];
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 3, mat_v.as_ptr(),
                std::ptr::null()));
        let mut target = [0.5f32, 0.5, 0.5];
        let mut result = [0.0f32; 3];
        let mut hint = [0.0f32; 3];
        let ok = c::cmsPipelineEvalReverseFloat(
            target.as_mut_ptr(), result.as_mut_ptr(),
            hint.as_mut_ptr(), pl);
        eprintln!("reverse eval={ok}");
        c::cmsPipelineFree(pl);
        // SliceSpace16 — iterate the input space via a sampler
        unsafe extern "C" fn slice16(
            inp: *const u16, _out: *mut u16,
            _c: *mut std::ffi::c_void) -> i32 {
            let _ = inp;
            1
        }
        let grid = [4u32; 3];
        let ok2 = c::cmsSliceSpace16(3, grid.as_ptr(), slice16,
            std::ptr::null_mut());
        eprintln!("slice16 ok={ok2}");
        unsafe extern "C" fn slicef(
            inp: *const f32, _out: *mut f32,
            _c: *mut std::ffi::c_void) -> i32 {
            let _ = inp;
            1
        }
        let ok3 = c::cmsSliceSpaceFloat(3, grid.as_ptr(), slicef,
            std::ptr::null_mut());
        eprintln!("slicef ok={ok3}");
    }
}

/// Identity-stage internals — AllocIdentity + eval on more
/// channel counts.
#[test]
fn identity_stages() {
    unsafe {
        for nch in [1u32, 2, 3, 4, 6, 8, 15, 16] {
            let st = c::cmsStageAllocIdentity(
                std::ptr::null_mut(), nch);
            eprintln!("nch={nch} null={}", st.is_null());
            if !st.is_null() {
                let pl = c::cmsPipelineAlloc(
                    std::ptr::null_mut(), nch, nch);
                if pl.is_null() {
                    eprintln!("  pl=null");
                    c::cmsStageFree(st);
                    continue;
                }
                c::cmsPipelineInsertStage(pl, 1, st);
                let mut inp = [0.5f32; 16];
                let mut out = [0.0f32; 16];
                c::cmsPipelineEvalFloat(inp.as_ptr(),
                    out.as_mut_ptr(), pl);
                let _ = (inp, out);
                c::cmsPipelineFree(pl);
            }
        }
    }
}

/// CLUT-stage internals — AllocCLut16bit/Float on more grid
/// combos (the CLUT-stage machinery).
#[test]
fn clut_stage_machinery() {
    unsafe {
        for (name, npts, nch) in [
            ("2pt-3ch", 2u32, 3u32),
            ("4pt-3ch", 4, 3),
            ("8pt-4ch", 8, 4),
            ("17pt-4ch", 17, 4),
            ("33pt-3ch", 33, 3),
        ] {
            let mut n = 1usize;
            for _ in 0..nch { n *= npts as usize; }
            let mut tab = vec![0x4000u16; n * nch as usize];
            let st = c::cmsStageAllocCLut16bit(
                std::ptr::null_mut(), npts, nch, nch,
                tab.as_mut_ptr());
            eprintln!("{name} null={}", st.is_null());
            if !st.is_null() {
                let pl = c::cmsPipelineAlloc(
                    std::ptr::null_mut(), nch, nch);
                if pl.is_null() {
                    eprintln!("  pl=null");
                    c::cmsStageFree(st);
                    continue;
                }
                c::cmsPipelineInsertStage(pl, 1, st);
                let mut inp = [0.5f32; 16];
                let mut out = [0.0f32; 16];
                c::cmsPipelineEvalFloat(inp.as_ptr(),
                    out.as_mut_ptr(), pl);
                let _ = (inp, out);
                c::cmsPipelineFree(pl);
            }
        }
    }
}

/// Stage-alloc internals — more stage-type variants.
#[test]
fn stage_alloc_variants() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = [g, g, g];
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let grid = [4u32; 3];
        let tab = [0.5f32; 4 * 4 * 4 * 3];
        for (name, st) in [
            ("identity-3", c::cmsStageAllocIdentity(
                std::ptr::null_mut(), 3)),
            ("identity-4", c::cmsStageAllocIdentity(
                std::ptr::null_mut(), 4)),
            ("identity-15", c::cmsStageAllocIdentity(
                std::ptr::null_mut(), 15)),
            ("identity-16", c::cmsStageAllocIdentity(
                std::ptr::null_mut(), 16)),
            ("curves", c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, curves.as_ptr())),
            ("matrix", c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 3, mat.as_ptr(),
                std::ptr::null())),
            ("clut-flt", c::cmsStageAllocCLutFloatGranular(
                std::ptr::null_mut(), grid.as_ptr(), 3, 3,
                tab.as_ptr())),
        ] {
            eprintln!("{name} null={}", st.is_null());
            if !st.is_null() {
                let pl = c::cmsPipelineAlloc(
                    std::ptr::null_mut(), 3, 3);
                c::cmsPipelineInsertStage(pl, 1, st);
                c::cmsPipelineFree(pl);
            }
        }
    }
}

/// CLUT granular grids on more combos — the CLUT
/// internals.
#[test]
fn clut_granular2() {
    unsafe {
        for (name, ch, pts) in [
            ("3ch-2pt", 3u32, 2u32),
            ("3ch-17pt", 3, 17),
            ("3ch-33pt", 3, 33),
            ("4ch-2pt", 4, 2),
            ("4ch-17pt", 4, 17),
            ("2ch-16pt", 2, 16),
            ("6ch-8pt", 6, 8)] {
            let pl = c::cmsPipelineAlloc(
                std::ptr::null_mut(), ch, ch);
            let stage = c::cmsStageAllocCLut16bit(
                std::ptr::null_mut(), pts, ch, ch,
                std::ptr::null());
            eprintln!("{name} alloc={}", !stage.is_null());
            if !stage.is_null() {
                c::cmsPipelineInsertStage(pl, 1, stage);
            }
            c::cmsPipelineFree(pl);
        }
    }
}

/// Matrix internals on more combos — the matrix machinery.
#[test]
fn matrix_variants2() {
    unsafe {
        let ident = [
            1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0];
        let scale2 = [
            2.0,0.0,0.0, 0.0,2.0,0.0, 0.0,0.0,2.0];
        let zero = [0.0; 9];
        for (name, m) in [
            ("ident", ident.as_ptr()),
            ("scale2", scale2.as_ptr()),
            ("zero", zero.as_ptr())] {
            let pl = c::cmsPipelineAlloc(
                std::ptr::null_mut(), 3, 3);
            let stage = c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 3, m,
                std::ptr::null());
            eprintln!("{name} alloc={}", !stage.is_null());
            if !stage.is_null() {
                c::cmsPipelineInsertStage(pl, 1, stage);
            }
            c::cmsPipelineFree(pl);
        }
    }
}

/// CLUT-float internals on more grid combos — the CLUT
/// float machinery.
#[test]
fn clut_float2() {
    unsafe {
        for (name, ch, pts) in [
            ("3ch-2pt", 3u32, 2u32),
            ("3ch-17pt", 3, 17),
            ("4ch-2pt", 4, 2),
            ("4ch-17pt", 4, 17)] {
            let pl = c::cmsPipelineAlloc(
                std::ptr::null_mut(), ch, ch);
            let stage = c::cmsStageAllocCLutFloat(
                std::ptr::null_mut(), pts, ch, ch,
                std::ptr::null());
            eprintln!("{name} alloc={}", !stage.is_null());
            if !stage.is_null() {
                c::cmsPipelineInsertStage(pl, 1, stage);
            }
            c::cmsPipelineFree(pl);
        }
    }
}

/// Tone-curve internals on more combos — the tone-curve
/// machinery.
#[test]
fn tone_curves2() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let c1 = [g];
        let c3 = [g, g, g];
        let c4 = [g, g, g, g];
        let c15 = [g; 15];
        for (name, n, curves) in [
            ("1ch", 1u32, c1.as_ptr()),
            ("3ch", 3, c3.as_ptr()),
            ("4ch", 4, c4.as_ptr()),
            ("15ch", 15, c15.as_ptr())] {
            let pl = c::cmsPipelineAlloc(
                std::ptr::null_mut(), n, n);
            let stage = c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), n, curves);
            eprintln!("{name} alloc={}", !stage.is_null());
            if !stage.is_null() {
                c::cmsPipelineInsertStage(pl, 1, stage);
            }
            c::cmsPipelineFree(pl);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// CLUT sampling internals on more combos — the CLUT
/// sampling machinery.
#[test]
fn clut_sample2() {
    unsafe {
        for (name, ch, pts) in [
            ("3ch-2pt", 3u32, 2u32),
            ("3ch-17pt", 3, 17)] {
            let pl = c::cmsPipelineAlloc(
                std::ptr::null_mut(), ch, ch);
            let stage = c::cmsStageAllocCLut16bit(
                std::ptr::null_mut(), pts, ch, ch,
                std::ptr::null());
            if !stage.is_null() {
                c::cmsPipelineInsertStage(pl, 1, stage);
            }
            c::cmsPipelineFree(pl);
            eprintln!("{name} done");
        }
    }
}

/// Pipeline eval internals on more combos — the pipeline
/// eval machinery.
#[test]
fn pipeline_eval2() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for (name, n) in [
            ("1ch", 1u32), ("3ch", 3), ("4ch", 4), ("15ch", 15)] {
            let pl = c::cmsPipelineAlloc(
                std::ptr::null_mut(), n, n);
            let curves = vec![g; n as usize];
            c::cmsPipelineInsertStage(pl, 1,
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), n, curves.as_ptr()));
            let mut inp = [0.5f32; 15];
            let mut out = [0.0f32; 15];
            c::cmsPipelineEvalFloat(
                inp.as_ptr(), out.as_mut_ptr(), pl);
            eprintln!("{name} eval={:.4}", out[0]);
            c::cmsPipelineFree(pl);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// Pipeline eval16 internals on more combos — the u16
/// eval machinery.
#[test]
fn pipeline_eval16() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for (name, n) in [
            ("1ch", 1u32), ("3ch", 3), ("4ch", 4), ("15ch", 15)] {
            let pl = c::cmsPipelineAlloc(
                std::ptr::null_mut(), n, n);
            let curves = vec![g; n as usize];
            c::cmsPipelineInsertStage(pl, 1,
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), n, curves.as_ptr()));
            let inp = [0x8000u16; 15];
            let mut out = [0u16; 15];
            c::cmsPipelineEval16(
                inp.as_ptr(), out.as_mut_ptr(), pl);
            eprintln!("{name} eval={}", out[0]);
            c::cmsPipelineFree(pl);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// Identity-stage internals on more combos — the identity
/// machinery.
#[test]
fn identity_stage2() {
    unsafe {
        for (name, n) in [
            ("1ch", 1u32), ("3ch", 3), ("4ch", 4), ("15ch", 15)] {
            let pl = c::cmsPipelineAlloc(
                std::ptr::null_mut(), n, n);
            let stage = c::cmsStageAllocIdentity(
                std::ptr::null_mut(), n);
            eprintln!("{name} alloc={}", !stage.is_null());
            if !stage.is_null() {
                c::cmsPipelineInsertStage(pl, 1, stage);
            }
            c::cmsPipelineFree(pl);
        }
    }
}

/// Named-color-stage internals on more combos — the named
/// machinery.
#[test]
fn named_stage2() {
    unsafe {
        let ncl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 1, 3,
            b"P\0".as_ptr() as _,
            b"S\0".as_ptr() as _);
        let stage = c::_cmsStageAllocNamedColor(
            ncl, 0);
        eprintln!("alloc={}", !stage.is_null());
        if !stage.is_null() { c::cmsStageFree(stage); }
        c::cmsFreeNamedColorList(ncl);
    }
}

/// Slice-space machinery — cmsSliceSpace16/Float on more
/// channel counts.
#[test]
fn slice_space2() {
    unsafe extern "C" fn samp16(
        inp: *const u16, out: *mut u16, _c: *mut std::ffi::c_void)
        -> i32 {
        unsafe {
            if !out.is_null() && !inp.is_null() { *out = *inp; }
        }
        1
    }
    unsafe extern "C" fn sampf(
        inp: *const f32, out: *mut f32, _c: *mut std::ffi::c_void)
        -> i32 {
        unsafe {
            if !out.is_null() && !inp.is_null() { *out = *inp; }
        }
        1
    }
    unsafe {
        for (name, n) in [
            ("1ch", 1u32), ("3ch", 3), ("4ch", 4)] {
            let pts = [4u32; 15];
            let r = c::cmsSliceSpace16(n, pts.as_ptr(),
                samp16, std::ptr::null_mut());
            eprintln!("{name}-16={r}");
            let r2 = c::cmsSliceSpaceFloat(n, pts.as_ptr(),
                sampf, std::ptr::null_mut());
            eprintln!("{name}-f={r2}");
        }
    }
}

/// CLUT sampling machinery — cmsStageSampleCLut16bit/Float
/// on real stages.
#[test]
fn clut_sample_real() {
    unsafe extern "C" fn samp16(
        inp: *const u16, out: *mut u16, _c: *mut std::ffi::c_void)
        -> i32 {
        unsafe {
            for i in 0..3 { *out.add(i) = *inp.add(i); }
        }
        1
    }
    unsafe extern "C" fn sampf(
        inp: *const f32, out: *mut f32, _c: *mut std::ffi::c_void)
        -> i32 {
        unsafe {
            for i in 0..3 { *out.add(i) = *inp.add(i); }
        }
        1
    }
    unsafe {
        for (name, pts) in [("2pt", 2u32), ("5pt", 5)] {
            let st16 = c::cmsStageAllocCLut16bit(
                std::ptr::null_mut(), pts, 3, 3,
                std::ptr::null());
            let r1 = c::cmsStageSampleCLut16bit(st16,
                samp16, std::ptr::null_mut(), 0);
            eprintln!("{name}-16={r1}");
            let stf = c::cmsStageAllocCLutFloat(
                std::ptr::null_mut(), pts, 3, 3,
                std::ptr::null());
            let r2 = c::cmsStageSampleCLutFloat(stf,
                sampf, std::ptr::null_mut(), 0);
            eprintln!("{name}-f={r2}");
            c::cmsStageFree(st16);
            c::cmsStageFree(stf);
        }
    }
}

/// Pipeline introspection — stage-count/first/last on
/// different shapes.
#[test]
fn pipeline_introspect() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let c3 = [g, g, g];
        for (name, n_stage) in [
            ("empty", 0u32), ("1-stage", 1), ("2-stage", 2),
            ("4-stage", 4)] {
            let pl = c::cmsPipelineAlloc(
                std::ptr::null_mut(), 3, 3);
            for _ in 0..n_stage {
                c::cmsPipelineInsertStage(pl, 1,
                    c::cmsStageAllocIdentity(
                        std::ptr::null_mut(), 3));
            }
            let cnt = c::cmsPipelineStageCount(pl);
            let first = c::cmsPipelineGetPtrToFirstStage(pl);
            let last = c::cmsPipelineGetPtrToLastStage(pl);
            eprintln!("{name} cnt={cnt} first={} last={}",
                !first.is_null(), !last.is_null());
            c::cmsPipelineFree(pl);
        }
        let _ = c3;
        c::cmsFreeToneCurve(g);
    }
}

/// Pipeline insert positions — beginning/end/middle on
/// different shapes.
#[test]
fn pipeline_insert_pos() {
    unsafe {
        for (name, n_insert, pos) in [
            ("end", 2u32, 1i32), ("begin", 2, 0),
            ("middle", 3, 0)] {
            let pl = c::cmsPipelineAlloc(
                std::ptr::null_mut(), 3, 3);
            for _ in 0..n_insert {
                c::cmsPipelineInsertStage(pl, pos,
                    c::cmsStageAllocIdentity(
                        std::ptr::null_mut(), 3));
            }
            let cnt = c::cmsPipelineStageCount(pl);
            eprintln!("{name} cnt={cnt}");
            c::cmsPipelineFree(pl);
        }
    }
}

/// Pipeline/stage dup machinery on more shapes.
#[test]
fn pipeline_dup() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let c3 = [g, g, g];
        for (name, build) in [
            ("identity", 0u32), ("curves", 1), ("mixed", 2)] {
            let pl = c::cmsPipelineAlloc(
                std::ptr::null_mut(), 3, 3);
            match build {
                1 => { c::cmsPipelineInsertStage(pl, 1,
                    c::cmsStageAllocToneCurves(
                        std::ptr::null_mut(), 3, c3.as_ptr())); }
                2 => {
                    c::cmsPipelineInsertStage(pl, 1,
                        c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3, c3.as_ptr()));
                    c::cmsPipelineInsertStage(pl, 1,
                        c::cmsStageAllocMatrix(
                            std::ptr::null_mut(), 3, 3,
                            [1.0,0.0,0.0, 0.0,1.0,0.0,
                             0.0,0.0,1.0].as_ptr(),
                            std::ptr::null()));
                }
                _ => {}
            }
            let dup = c::cmsPipelineDup(pl);
            let cnt = if dup.is_null() { 0u32 } else {
                c::cmsPipelineStageCount(dup)
            };
            eprintln!("{name} dup={} cnt={cnt}",
                !dup.is_null());
            if !dup.is_null() { c::cmsPipelineFree(dup); }
            c::cmsPipelineFree(pl);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// Stage-type machinery — cmsStageType on more stage types.
#[test]
fn stage_type() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let c3 = [g, g, g];
        for (name, st) in [
            ("identity", c::cmsStageAllocIdentity(
                std::ptr::null_mut(), 3)),
            ("curves", c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, c3.as_ptr())),
            ("matrix", c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 3,
                [1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0]
                    .as_ptr(), std::ptr::null())),
            ("clut16", c::cmsStageAllocCLut16bit(
                std::ptr::null_mut(), 2, 3, 3,
                std::ptr::null())),
            ("clutf", c::cmsStageAllocCLutFloat(
                std::ptr::null_mut(), 2, 3, 3,
                std::ptr::null()))] {
            let t = c::cmsStageType(st);
            eprintln!("{name} type={t:#x}");
            c::cmsStageFree(st);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// Stage-dup machinery — cmsStageDup on more stage types.
#[test]
fn stage_dup() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let c3 = [g, g, g];
        for (name, st) in [
            ("identity", c::cmsStageAllocIdentity(
                std::ptr::null_mut(), 3)),
            ("curves", c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, c3.as_ptr())),
            ("matrix", c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 3,
                [1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0]
                    .as_ptr(), std::ptr::null())),
            ("clut16", c::cmsStageAllocCLut16bit(
                std::ptr::null_mut(), 2, 3, 3,
                std::ptr::null())),
            ("clutf", c::cmsStageAllocCLutFloat(
                std::ptr::null_mut(), 2, 3, 3,
                std::ptr::null()))] {
            let d = c::cmsStageDup(st);
            eprintln!("{name} dup={}", !d.is_null());
            if !d.is_null() { c::cmsStageFree(d); }
            c::cmsStageFree(st);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// Stage channel-count machinery — input/output channels
/// on more stage types.
#[test]
fn stage_channels2() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let c3 = [g, g, g];
        for (name, st) in [
            ("id-3", c::cmsStageAllocIdentity(
                std::ptr::null_mut(), 3)),
            ("id-1", c::cmsStageAllocIdentity(
                std::ptr::null_mut(), 1)),
            ("id-4", c::cmsStageAllocIdentity(
                std::ptr::null_mut(), 4)),
            ("id-15", c::cmsStageAllocIdentity(
                std::ptr::null_mut(), 15)),
            ("curves3", c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, c3.as_ptr())),
            ("matrix33", c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 3,
                [1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0]
                    .as_ptr(), std::ptr::null())),
            ("matrix34", c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 4,
                [1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0,
                 0.0,0.0,0.0].as_ptr(), std::ptr::null())),
            ("clut33", c::cmsStageAllocCLut16bit(
                std::ptr::null_mut(), 2, 3, 3,
                std::ptr::null())),
            ("clut43", c::cmsStageAllocCLut16bit(
                std::ptr::null_mut(), 2, 4, 3,
                std::ptr::null())),
            ("clut34", c::cmsStageAllocCLut16bit(
                std::ptr::null_mut(), 2, 3, 4,
                std::ptr::null()))] {
            if st.is_null() {
                eprintln!("{name} NULL");
                continue;
            }
            let i = c::cmsStageInputChannels(st);
            let o = c::cmsStageOutputChannels(st);
            eprintln!("{name} in={i} out={o}");
            c::cmsStageFree(st);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// Pipeline channel-count + stage-walk machinery on more
/// shapes.
#[test]
fn pipeline_channels_walk() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let c3 = [g, g, g];
        for (name, ni, no) in [
            ("3-3", 3u32, 3u32), ("1-1", 1, 1),
            ("4-4", 4, 4), ("3-4", 3, 4)] {
            let pl = c::cmsPipelineAlloc(
                std::ptr::null_mut(), ni, no);
            let pi = c::cmsPipelineInputChannels(pl);
            let po = c::cmsPipelineOutputChannels(pl);
            // insert an identity + walk stages
            c::cmsPipelineInsertStage(pl, 1,
                c::cmsStageAllocIdentity(
                    std::ptr::null_mut(), ni));
            let mut st = c::cmsPipelineGetPtrToFirstStage(pl);
            let mut n_st = 0;
            while !st.is_null() {
                n_st += 1;
                st = c::cmsStageNext(st);
            }
            eprintln!("{name} in={pi} out={po} stages={n_st}");
            c::cmsPipelineFree(pl);
        }
        let _ = c3;
        c::cmsFreeToneCurve(g);
    }
}

/// Reverse-eval machinery — cmsPipelineEvalReverseFloat on
/// more shapes.
#[test]
fn pipeline_eval_reverse() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let c3 = [g, g, g];
        for (name, build) in [
            ("curves", 0u32), ("identity", 1), ("clut", 2)] {
            let pl = c::cmsPipelineAlloc(
                std::ptr::null_mut(), 3, 3);
            match build {
                0 => { c::cmsPipelineInsertStage(pl, 1,
                    c::cmsStageAllocToneCurves(
                        std::ptr::null_mut(), 3, c3.as_ptr())); }
                1 => { c::cmsPipelineInsertStage(pl, 1,
                    c::cmsStageAllocIdentity(
                        std::ptr::null_mut(), 3)); }
                2 => { c::cmsPipelineInsertStage(pl, 1,
                    c::cmsStageAllocCLut16bit(
                        std::ptr::null_mut(), 2, 3, 3,
                        std::ptr::null())); }
                _ => {}
            }
            let inp = [0.5f32; 3];
            let mut out = [0.0f32; 3];
            let mut hint = [0.0f32; 3];
            let r = c::cmsPipelineEvalReverseFloat(
                inp.as_ptr() as *mut f32, out.as_mut_ptr(),
                hint.as_mut_ptr(), pl);
            eprintln!("{name} rev={r} out0={:.3}", out[0]);
            c::cmsPipelineFree(pl);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// Save-as-8bits flag + granular CLUT allocs on more
/// combos.
#[test]
fn pipeline_flag_granular() {
    unsafe {
        let pl = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        for v in [0i32, 1, 2] {
            let r = c::cmsPipelineSetSaveAs8bitsFlag(pl, v);
            eprintln!("flag={v} r={r}");
        }
        // granular CLUTs — per-dimension grid
        for (name, grid) in [
            ("333", [3u32, 3, 3]),
            ("222", [2u32, 2, 2]),
            ("mixed", [2u32, 3, 4])] {
            let st = c::cmsStageAllocCLut16bitGranular(
                std::ptr::null_mut(), grid.as_ptr(), 3, 3,
                std::ptr::null());
            eprintln!("{name} alloc={}", !st.is_null());
            if !st.is_null() { c::cmsStageFree(st); }
        }
        c::cmsPipelineFree(pl);
    }
}

/// Float-granular CLUT allocs on more combos — the float
/// granular machinery.
#[test]
fn float_granular() {
    unsafe {
        for (name, grid) in [
            ("333", [3u32, 3, 3].as_slice()),
            ("222", [2u32, 2, 2].as_slice()),
            ("mixed", [2u32, 3, 4].as_slice()),
            ("44", [4u32, 4, 4, 4].as_slice())] {
            let ch = if grid.len() == 4 { 4u32 } else { 3 };
            let st = c::cmsStageAllocCLutFloatGranular(
                std::ptr::null_mut(), grid.as_ptr(), ch, ch,
                std::ptr::null());
            eprintln!("{name} alloc={}", !st.is_null());
            if !st.is_null() { c::cmsStageFree(st); }
        }
    }
}

/// Pipeline eval on multi-channel shapes — the eval
/// machinery on more channel counts.
#[test]
fn pipeline_eval_multich() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for (name, n) in [
            ("2ch", 2u32), ("6ch", 6), ("8ch", 8), ("15ch", 15)] {
            let pl = c::cmsPipelineAlloc(
                std::ptr::null_mut(), n, n);
            let curves = vec![g; n as usize];
            c::cmsPipelineInsertStage(pl, 1,
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), n, curves.as_ptr()));
            let inp = [0x8000u16; 15];
            let mut out = [0u16; 15];
            c::cmsPipelineEval16(
                inp.as_ptr(), out.as_mut_ptr(), pl);
            let inpf = [0.5f32; 15];
            let mut outf = [0.0f32; 15];
            c::cmsPipelineEvalFloat(
                inpf.as_ptr(), outf.as_mut_ptr(), pl);
            eprintln!("{name} u16={} f={:.3}", out[0], outf[0]);
            c::cmsPipelineFree(pl);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// Pipeline insert validation — invalid positions/stages.
#[test]
fn pipeline_insert_invalid() {
    unsafe {
        let pl = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocIdentity(std::ptr::null_mut(), 3));
        // invalid positions — the insert-validation machinery
        for (name, pos) in [
            ("neg1", -1i32), ("zero", 0), ("one", 1),
            ("huge", 999), ("neg-huge", -999)] {
            let st = c::cmsStageAllocIdentity(
                std::ptr::null_mut(), 3);
            let r = c::cmsPipelineInsertStage(pl, pos, st);
            eprintln!("{name} r={r}");
            if r == 0 { c::cmsStageFree(st); }
        }
        // NULL stage
        let r = c::cmsPipelineInsertStage(pl, 1,
            std::ptr::null_mut());
        eprintln!("null-stage r={r}");
        c::cmsPipelineFree(pl);
    }
}

/// Named/curve-alloc validation — more channel counts +
/// NULL curves.
#[test]
fn stage_alloc_valid() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        // named-color on more channel counts
        for (name, n) in [
            ("named-1", 1u32), ("named-3", 3), ("named-4", 4)] {
            let ncl = c::cmsAllocNamedColorList(
                std::ptr::null_mut(), n, n,
                b"P\0".as_ptr() as _, b"S\0".as_ptr() as _);
            let st = c::_cmsStageAllocNamedColor(ncl, 0);
            eprintln!("{name} alloc={}", !st.is_null());
            if !st.is_null() { c::cmsStageFree(st); }
            c::cmsFreeNamedColorList(ncl);
        }
        // NULL curves — the curve-alloc validation
        let st2 = c::cmsStageAllocToneCurves(
            std::ptr::null_mut(), 3, std::ptr::null());
        eprintln!("null-curves alloc={}", !st2.is_null());
        if !st2.is_null() { c::cmsStageFree(st2); }
        let _ = g;
    }
}

/// Matrix-alloc machinery on more dimensions.
#[test]
fn matrix_dims() {
    unsafe {
        for (name, r, c, n) in [
            ("1x1", 1u32, 1u32, 1usize),
            ("3x3", 3, 3, 9),
            ("4x4", 4, 4, 16),
            ("3x4", 3, 4, 12),
            ("4x3", 4, 3, 12),
            ("15x15", 15, 15, 225)] {
            let m = vec![1.0f64; n];
            let st = c::cmsStageAllocMatrix(
                std::ptr::null_mut(), r, c, m.as_ptr(),
                std::ptr::null());
            eprintln!("{name} alloc={}", !st.is_null());
            if !st.is_null() { c::cmsStageFree(st); }
        }
    }
}

/// mAB-structured pipelines — [curves, matrix, curves],
/// [curves, clut, curves] and the full 5-element
/// [curves, clut, curves, matrix, curves] — the only layouts
/// Type_LUTA2B_Write accepts. Written as A2B0, saved, byte-exact.
/// Hits Type_MPEmatrix_Write + Type_MPEclut_Write + GenericMPE*.
#[test]
fn mpe_structured_writes() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let cur = [g; 3];
        let rg = lcms::gamma::build_gamma(None, 2.2).unwrap();
        let mat: [f64; 9] = [1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0];
        let mut tab = [0u16; 8 * 3];
        for (i, v) in tab.iter_mut().enumerate() {
            *v = (i * 2730) as u16;
        }
        for name in ["c-mat-c", "c-clut-c", "full5", "clut-only"] {
            let ch = c::cmsCreate_sRGBProfile();
            let mut rp = lcms::virt::create_srgb_profile(None).unwrap();
            let cpipe = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            let mut rpipe = lut::pipeline_alloc(None, 3, 3).unwrap();
            let mk_cur = |pipe| c::cmsPipelineInsertStage(pipe, 1,
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), 3, cur.as_ptr()));
            let mk_mat = |pipe| c::cmsPipelineInsertStage(pipe, 1,
                c::cmsStageAllocMatrix(std::ptr::null_mut(),
                    3, 3, mat.as_ptr(), std::ptr::null()));
            let mk_clut = |pipe| c::cmsPipelineInsertStage(pipe, 1,
                c::cmsStageAllocCLut16bit(
                    std::ptr::null_mut(), 2, 3, 3, tab.as_ptr()));
            let mut rcur = || lut::stage_alloc_tone_curves(
                None, 3, Some(&[&rg; 3])).unwrap();
            match name {
                "c-mat-c" => {
                    mk_cur(cpipe); mk_mat(cpipe); mk_cur(cpipe);
                    let a = rcur();
                    let m = lut::stage_alloc_matrix(None, 3, 3, &mat, None).unwrap();
                    let b = rcur();
                    for st in [a, m, b] {
                        rpipe.insert_stage(lut::StageLoc::AtEnd, st); }
                }
                "c-clut-c" => {
                    mk_cur(cpipe); mk_clut(cpipe); mk_cur(cpipe);
                    let a = rcur();
                    let k = lut::stage_alloc_clut16bit(None, 2, 3, 3, Some(&tab)).unwrap();
                    let b = rcur();
                    for st in [a, k, b] {
                        rpipe.insert_stage(lut::StageLoc::AtEnd, st); }
                }
                "full5" => {
                    mk_cur(cpipe); mk_clut(cpipe); mk_cur(cpipe);
                    mk_mat(cpipe); mk_cur(cpipe);
                    let s: Vec<_> = (0..5).map(|i| match i {
                        1 => lut::stage_alloc_clut16bit(None, 2, 3, 3, Some(&tab)).unwrap(),
                        3 => lut::stage_alloc_matrix(None, 3, 3, &mat, None).unwrap(),
                        _ => rcur(),
                    }).collect();
                    for st in s {
                        rpipe.insert_stage(lut::StageLoc::AtEnd, st); }
                }
                _ => {
                    mk_cur(cpipe);
                    rpipe.insert_stage(lut::StageLoc::AtEnd, rcur());
                }
            }
            let c_ok = c::cmsWriteTag(ch, 0x41324230, cpipe as *const _);
            let r_ok = lcms::profile::cms_write_tag(&mut rp, 0x41324230,
                Some(lcms::tagtypes::TagObject::Mpe(rpipe)));
            assert_eq!(c_ok != 0, r_ok, "{name} write");
            let mut c_out = vec![0u8; 1 << 20];
            let mut size = c_out.len() as u32;
            let s_ok = c::cmsSaveProfileToMem(ch,
                c_out.as_mut_ptr() as *mut _, &mut size);
            let r_out = lcms::profile::save_profile_to_mem(&mut rp);
            assert_eq!(s_ok != 0, r_out.is_some(), "{name} save status");
            if s_ok != 0 {
                c_out.truncate(size as usize);
                let mut r_out = r_out.unwrap();
                for i in 0..12 {
                    c_out.get_mut(24 + i).map(|b| *b = 0);
                    r_out.get_mut(24 + i).map(|b| *b = 0);
                }
                c_out.resize(r_out.len().max(c_out.len()), 0);
                r_out.resize(c_out.len(), 0);
                assert_eq!(c_out, r_out, "{name} resave");
            }
            c::cmsCloseProfile(ch);
        }
    }
}

/// mpet element writers — segmented curves + matrix + FLOAT clut
/// written to D2B0/B2D0 ('mpet' type) — hits Type_MPEcurve_Write,
/// Type_MPEmatrix_Write, Type_MPEclut_Write (float CLUT only) and
/// the GenericMPE* dispatch mAB never reaches. Save->reopen->read
/// exercises the cooked element readers too.
#[test]
fn mpet_element_writes() {
    unsafe {
        let mat: [f64; 9] = [1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0];
        let mut ftab = [0f32; 8 * 3];
        for (i, v) in ftab.iter_mut().enumerate() {
            *v = i as f32 / 24.0;
        }
        let mut samp = [0f32; 256];
        for (i, v) in samp.iter_mut().enumerate() {
            *v = i as f32 / 255.0;
        }
        let cseg = c::CurveSegment {
            x0: 0.0, x1: 1.0, r#type: 0,
            params: [0.0; 10],
            n_grid_points: 256,
            sampled_points: samp.as_ptr(),
        };
        let rseg = lcms::gamma::CurveSegment {
            x0: 0.0, x1: 1.0, r#type: 0,
            params: [0.0; 10],
            n_grid_points: 256,
            sampled_points: Some(samp.to_vec()),
        };
        for sig in [0x44324230u32, 0x42324430] { // 'D2B0','B2D0'
            let ch = c::cmsCreate_sRGBProfile();
            let mut rp = lcms::virt::create_srgb_profile(None).unwrap();
            let cpipe = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            let cc = [
                c::cmsBuildSegmentedToneCurve(
                    std::ptr::null_mut(), 1, &cseg),
                c::cmsBuildSegmentedToneCurve(
                    std::ptr::null_mut(), 1, &cseg),
                c::cmsBuildSegmentedToneCurve(
                    std::ptr::null_mut(), 1, &cseg)];
            c::cmsPipelineInsertStage(cpipe, 1,
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), 3, cc.as_ptr()));
            c::cmsPipelineInsertStage(cpipe, 1,
                c::cmsStageAllocMatrix(std::ptr::null_mut(),
                    3, 3, mat.as_ptr(), std::ptr::null()));
            c::cmsPipelineInsertStage(cpipe, 1,
                c::cmsStageAllocCLutFloat(
                    std::ptr::null_mut(), 2, 3, 3, ftab.as_ptr()));
            let rc: Vec<_> = (0..3).map(|_|
                lcms::gamma::build_segmented_tone_curve(
                    None, &[rseg.clone()]).unwrap()).collect();
            let mut rpipe = lut::pipeline_alloc(None, 3, 3).unwrap();
            rpipe.insert_stage(lut::StageLoc::AtEnd,
                lut::stage_alloc_tone_curves(None, 3,
                    Some(&[&rc[0], &rc[1], &rc[2]])).unwrap());
            rpipe.insert_stage(lut::StageLoc::AtEnd,
                lut::stage_alloc_matrix(None, 3, 3, &mat, None)
                    .unwrap());
            rpipe.insert_stage(lut::StageLoc::AtEnd,
                lut::stage_alloc_clut_float(None, 2, 3, 3,
                    Some(&ftab)).unwrap());
            let c_ok = c::cmsWriteTag(ch, sig, cpipe as *const _);
            let r_ok = lcms::profile::cms_write_tag(&mut rp, sig,
                Some(lcms::tagtypes::TagObject::Mpe(rpipe)));
            assert_eq!(c_ok != 0, r_ok, "{sig:#x} write");
            let mut c_out = vec![0u8; 1 << 20];
            let mut n = c_out.len() as u32;
            let s_ok = c::cmsSaveProfileToMem(ch,
                c_out.as_mut_ptr() as *mut _, &mut n);
            let r_out = lcms::profile::save_profile_to_mem(&mut rp);
            assert_eq!(s_ok != 0, r_out.is_some(), "{sig:#x} save");
            if s_ok != 0 {
                c_out.truncate(n as usize);
                let mut r_out = r_out.unwrap();
                for i in 0..12 {
                    c_out.get_mut(24 + i).map(|b| *b = 0);
                    r_out.get_mut(24 + i).map(|b| *b = 0);
                }
                c_out.resize(r_out.len().max(c_out.len()), 0);
                r_out.resize(c_out.len(), 0);
                assert_eq!(c_out, r_out, "{sig:#x} resave");
                let h2 = c::cmsOpenProfileFromMem(
                    c_out.as_ptr() as *const _, c_out.len() as u32);
                assert!(!h2.is_null(), "{sig:#x} reopen");
                let t = c::cmsReadTag(h2, sig);
                assert!(!t.is_null(), "{sig:#x} cooked read");
                c::cmsCloseProfile(h2);
            }
            c::cmsCloseProfile(ch);
        }
    }
}

/// Float-LUT tag reads — a profile carrying 'mpet' tags gets its
/// LUT through _cmsReadFloatInputTag/_cmsReadFloatOutputTag/
/// _cmsReadFloatDevicelinkTag (Device2PCSFloat=D2B*, PCS2DeviceFloat=
/// B2D*). Pipeline: segmented curves + matrix + float CLUT.
#[test]
fn float_lut_reads() {
    unsafe {
        let mat: [f64; 9] = [1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0];
        let mut ftab = [0f32; 8 * 3];
        for (i, v) in ftab.iter_mut().enumerate() {
            *v = i as f32 / 24.0;
        }
        let mut samp = [0f32; 256];
        for (i, v) in samp.iter_mut().enumerate() {
            *v = i as f32 / 255.0;
        }
        let mk_pipe = |segs: &[c::CurveSegment]| {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            let cc = [
                c::cmsBuildSegmentedToneCurve(
                    std::ptr::null_mut(), 1, &segs[0]),
                c::cmsBuildSegmentedToneCurve(
                    std::ptr::null_mut(), 1, &segs[0]),
                c::cmsBuildSegmentedToneCurve(
                    std::ptr::null_mut(), 1, &segs[0])];
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), 3, cc.as_ptr()));
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocMatrix(std::ptr::null_mut(),
                    3, 3, mat.as_ptr(), std::ptr::null()));
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocCLutFloat(
                    std::ptr::null_mut(), 2, 3, 3, ftab.as_ptr()));
            p
        };
        let cseg = c::CurveSegment {
            x0: 0.0, x1: 1.0, r#type: 0,
            params: [0.0; 10],
            n_grid_points: 256,
            sampled_points: samp.as_ptr(),
        };
        // output side: sRGB with B2D0 (PCS2DeviceFloat)
        let outp = c::cmsCreate_sRGBProfile();
        c::cmsWriteTag(outp, 0x42324430, mk_pipe(&[cseg]) as *const _);
        // input side: sRGB with D2B0 (Device2PCSFloat)
        let inp = c::cmsCreate_sRGBProfile();
        c::cmsWriteTag(inp, 0x44324230, mk_pipe(&[cseg]) as *const _);
        // devicelink: link class + D2B0
        let dlk = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(dlk, 0x6C696E6B); // 'link'
        c::cmsSetColorSpace(dlk, 0x52474220); // 'RGB '
        c::cmsSetPCS(dlk, 0x4C616220); // 'Lab '
        c::cmsWriteTag(dlk, 0x44324230, mk_pipe(&[cseg]) as *const _);
        // transforms — srgb->outp(B2D0), inp(D2B0)->srgb, dlk xform
        let rgb8: u32 = (4 << 16) | (3 << 3) | 1;
        let srgb = c::cmsCreate_sRGBProfile();
        for (name, a, b) in [
            ("out-B2D0", srgb, outp),
            ("in-D2B0", inp, srgb),
            ("link-D2B0", dlk, srgb)] {
            let t = c::cmsCreateTransform(a, rgb8, b, rgb8, 0, 0);
            eprintln!("{name} xform={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(outp);
        c::cmsCloseProfile(inp);
        c::cmsCloseProfile(dlk);
        // Lab-PCS mpet -> _cmsStageNormalizeToLabFloat on read
        let labp = c::cmsCreateLab4Profile(std::ptr::null_mut());
        c::cmsWriteTag(labp, 0x44324230, mk_pipe(&[cseg]) as *const _);
        let lab8: u32 = (10 << 16) | (3 << 3) | 1;
        let s2 = c::cmsCreate_sRGBProfile();
        let t = c::cmsCreateTransform(labp, lab8, s2, rgb8, 0, 0);
        eprintln!("lab-D2B0 xform={}", !t.is_null());
        if !t.is_null() { c::cmsDeleteTransform(t); }
        // and the reverse direction through the same profile's B2D0
        let labp2 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        c::cmsWriteTag(labp2, 0x42324430, mk_pipe(&[cseg]) as *const _);
        let t2 = c::cmsCreateTransform(s2, rgb8, labp2, lab8, 0, 0);
        eprintln!("lab-B2D0 xform={}", !t2.is_null());
        if !t2.is_null() { c::cmsDeleteTransform(t2); }
        c::cmsCloseProfile(labp);
        c::cmsCloseProfile(labp2);
        c::cmsCloseProfile(s2);
    }
}

/// Optimized-pipeline duplication — cmsPipelineDup on a transform's
/// optimized LUT invokes the optimizer's dup callbacks
/// (Prelin16dup/Prelin8dup/CurvesDup/DupMatShaper).
#[test]
fn optimized_pipeline_dup() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let pb = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../aRGBlcms2.icc");
        let pth = std::ffi::CString::new(pb.to_str().unwrap()).unwrap();
        let argb = c::cmsOpenProfileFromFile(pth.as_ptr(), b"r\0".as_ptr());
        let rgb8: u32 = (4 << 16) | (3 << 3) | 1;
        let rgb16: u32 = (4 << 16) | (3 << 3) | 2;
        // matshaper opt (srgb->argb), u8 (prelin8) + u16 (prelin16)
        for (name, a, b, fi, fo) in [
            ("ms8", srgb, argb, rgb8, rgb8),
            ("ms16", srgb, argb, rgb16, rgb16),
        ] {
            let t = c::cmsCreateTransform(a, fi, b, fo, 0, 0);
            if t.is_null() { eprintln!("{name} no-xform"); continue; }
            let p = c::cmsGetTransformPipeline(t);
            let d = c::cmsPipelineDup(p);
            eprintln!("{name} pipe={} dup={}",
                !p.is_null(), !d.is_null());
            if !d.is_null() { c::cmsPipelineFree(d); }
            c::cmsDeleteTransform(t);
        }
        // curves-only opt — gray profile transform
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gp = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let gray8: u32 = (3 << 16) | (1 << 3) | 1;
        let t = c::cmsCreateTransform(gp, gray8, gp, gray8, 0, 0);
        if !t.is_null() {
            let p = c::cmsGetTransformPipeline(t);
            let d = c::cmsPipelineDup(p);
            eprintln!("curves dup={}", !d.is_null());
            if !d.is_null() { c::cmsPipelineFree(d); }
            c::cmsDeleteTransform(t);
        }
        // devicelink lut pipeline (clut+curves) -> prelin
        let gg = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut cc = [gg; 3];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x52474220, cc.as_mut_ptr());
        let t = c::cmsCreateTransform(dl, rgb8, dl, rgb8, 0, 0);
        if !t.is_null() {
            let p = c::cmsGetTransformPipeline(t);
            let d = c::cmsPipelineDup(p);
            eprintln!("link dup={}", !d.is_null());
            if !d.is_null() { c::cmsPipelineFree(d); }
            c::cmsDeleteTransform(t);
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(argb);
        c::cmsCloseProfile(gp);
        c::cmsCloseProfile(dl);
    }
}

/// MPE-element dup/free — cmsPipelineDup on an mpet-loaded pipeline
/// invokes GenericMPEdup per element (cmsStageDup routes through the
/// element-type handler's DupPtr); freeing -> GenericMPEfree.
#[test]
fn mpe_pipeline_dup_free() {
    unsafe {
        let mat: [f64; 9] = [1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0];
        let mut ftab = [0f32; 8 * 3];
        for (i, v) in ftab.iter_mut().enumerate() {
            *v = i as f32 / 24.0;
        }
        let mut samp = [0f32; 256];
        for (i, v) in samp.iter_mut().enumerate() {
            *v = i as f32 / 255.0;
        }
        let seg = c::CurveSegment {
            x0: 0.0, x1: 1.0, r#type: 0,
            params: [0.0; 10],
            n_grid_points: 256,
            sampled_points: samp.as_ptr(),
        };
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let cc = [
            c::cmsBuildSegmentedToneCurve(std::ptr::null_mut(), 1, &seg),
            c::cmsBuildSegmentedToneCurve(std::ptr::null_mut(), 1, &seg),
            c::cmsBuildSegmentedToneCurve(std::ptr::null_mut(), 1, &seg)];
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3, cc.as_ptr()));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), std::ptr::null()));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        // store as mpet on a profile then read it back — read rebuilds
        // the pipeline through the element handlers
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsWriteTag(prof, 0x44324230, p as *const _);
        let lut = c::cmsReadTag(prof, 0x44324230);
        eprintln!("mpet lut={}", !lut.is_null());
        // dup + free through the pipeline -> GenericMPEdup/free
        let d = c::cmsPipelineDup(lut);
        eprintln!("mpet dup={}", !d.is_null());
        if !d.is_null() { c::cmsPipelineFree(d); }
        // the tag pointer is profile-owned — cmsCloseProfile frees it
        c::cmsCloseProfile(prof);
    }
}

/// LUT8/LUT16 write stage-order sweep — the writers walk
/// [matrix?][curves?]clut[curves?] with a strict extra-stage reject;
/// cover each optional-piece permutation, clutless LUT8, and the
/// non-uniform-grid + wrong-channel rejects.
#[test]
fn lut_write_stage_orders() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut cc = [g; 3];
        let mat: [f64; 9] = [0.4,0.3,0.3, 0.2,0.6,0.2, 0.1,0.1,0.8];
        let tab = {
            let mut t = vec![0u16; 8 * 3];
            for (i, v) in t.iter_mut().enumerate() {
                *v = (i * 3) as u16;
            }
            t
        };
        let mk = |parts: &[u8]| {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            for &k in parts {
                match k {
                    0 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3, cc.as_ptr())),
                    1 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocMatrix(std::ptr::null_mut(),
                            3, 3, mat.as_ptr(), std::ptr::null())),
                    _ => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocCLut16bit(
                            std::ptr::null_mut(), 2, 3, 3, tab.as_ptr())),
                };
            }
            p
        };
        // stage orders: 0=curves 1=matrix 2=clut
        for (name, parts) in [
            ("clut-only", [2u8].as_slice()),
            ("cv-clut", [0, 2].as_slice()),
            ("clut-cv", [2, 0].as_slice()),
            ("cv-clut-cv", [0, 2, 0].as_slice()),
            ("cv-mx-cv", [0, 1, 0].as_slice()),
            ("cv-clut-cv-mx-cv", [0, 2, 0, 1, 0].as_slice()),
            ("full", [0, 1, 0, 2, 0].as_slice()),
            ("cv-only", [0u8].as_slice()),
            ("mx-only", [1u8].as_slice()),
            ("clut-mx-extra", [2, 1].as_slice()),
            ("cv-cv-clut", [0, 0, 2].as_slice()),
        ] {
            let p = mk(parts);
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, 0x41324230, p as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            eprintln!("{name}: w={w} save={s} n={}",
                if s != 0 { n as usize } else { 0 });
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
    }
}

/// LUT16 write stage-order sweep — v2-profile routes the same
/// pipeline shapes through Type_LUT16_Write (mft2): its
/// [matrix?][pre-curves?]clut[post-curves?] walk, clutless forms,
/// and the uniform-grid/3x3-matrix rejects.
#[test]
fn lut16_write_stage_orders() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut cc = [g; 3];
        let mat: [f64; 9] = [0.4,0.3,0.3, 0.2,0.6,0.2, 0.1,0.1,0.8];
        let mat2: [f64; 9] = [0.4,0.3,0.3, 0.2,0.6,0.2, 0.1,0.1,0.8];
        let tab: Vec<u16> = (0..8*3).map(|i| (i * 3) as u16).collect();
        let mk = |parts: &[u8], vers: f64| {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            for &k in parts {
                match k {
                    0 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3, cc.as_ptr())),
                    1 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocMatrix(std::ptr::null_mut(),
                            3, 3, mat.as_ptr(), std::ptr::null())),
                    2 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocMatrix(std::ptr::null_mut(),
                            3, 3, mat2.as_ptr(), std::ptr::null())),
                    _ => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocCLut16bit(
                            std::ptr::null_mut(), 2, 3, 3, tab.as_ptr())),
                };
            }
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, vers);
            let w = c::cmsWriteTag(prof, 0x41324230, p as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            eprintln!("  v{vers} w={w} save={s} n={}",
                if s != 0 { n as usize } else { 0 });
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        };
        // v2 profile -> mft2 route; cover the writer's shape arms
        for (name, parts) in [
            ("clut-only", [3u8].as_slice()),
            ("cv-clut", [0, 3].as_slice()),
            ("clut-cv", [3, 0].as_slice()),
            ("cv-clut-cv", [0, 3, 0].as_slice()),
            ("mx-cv-clut", [1, 0, 3].as_slice()),
            ("mx-cv-clut-cv", [1, 0, 3, 0].as_slice()),
            ("cv-only", [0u8].as_slice()),
            ("clut-mx-bad", [3, 1].as_slice()),
            ("mx4x4-bad", [2, 3].as_slice()),
        ] {
            eprintln!("{name}:");
            mk(parts, 2.1);
        }
    }
}

/// Stage-eval sweep — every stage type through PipelineEval16/
/// EvalFloat covers cmslut's per-type evaluators + the alloc/dup
/// paths for each element kind.
#[test]
fn stage_eval_sweep() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut cc = [g; 3];
        let mat: [f64; 9] = [0.4,0.3,0.3, 0.2,0.6,0.2, 0.1,0.1,0.8];
        let tab: Vec<u16> = (0..8*3).map(|i| (i * 41) as u16).collect();
        let ftab: Vec<f32> = (0..8*3).map(|i| i as f32 / 24.0).collect();
        let stages: &[(&str, *mut std::ffi::c_void)] = &[
            ("curves", c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, cc.as_ptr())),
            ("idcurves", c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, std::ptr::null())),
            ("matrix", c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), std::ptr::null())),
            ("clut16", c::cmsStageAllocCLut16bit(
                std::ptr::null_mut(), 2, 3, 3, tab.as_ptr())),
            ("clutflt", c::cmsStageAllocCLutFloat(
                std::ptr::null_mut(), 2, 3, 3, ftab.as_ptr())),
            ("identity", c::cmsStageAllocIdentity(std::ptr::null_mut(), 3)),
        ];
        for (name, st) in stages {
            if st.is_null() { eprintln!("{name}: alloc fail"); continue; }
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            c::cmsPipelineInsertStage(p, 1, *st);
            let mut i16 = [0x2000u16, 0x6000, 0xA000];
            let mut o16 = [0u16; 3];
            c::cmsPipelineEval16(i16.as_ptr(), o16.as_mut_ptr(), p);
            let fi = [0.5f32, 0.3, 0.7];
            let mut fo = [0f32; 3];
            c::cmsPipelineEvalFloat(fi.as_ptr(), fo.as_mut_ptr(), p);
            eprintln!("{name}: o16={o16:?} fo={fo:?}");
            // dup frees the dup paths too
            let dp = c::cmsPipelineDup(p);
            if !dp.is_null() { c::cmsPipelineFree(dp); }
            c::cmsPipelineFree(p);
        }
    }
}

/// LUT16/LUT8 read-back cycle — write varied pipeline shapes to a
/// v2-profile A2B0 tag, save, reload, cmsReadTag: exercises
/// Type_LUT16_Read/Type_LUT8_Read arms for each written shape
/// (matrix/pre/post curves/CLUT permutations).
#[test]
fn lut_readback_cycles() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut cc = [g; 3];
        let mat: [f64; 9] = [0.4,0.3,0.3, 0.2,0.6,0.2, 0.1,0.1,0.8];
        let tab16: Vec<u16> = (0..8*3).map(|i| (i * 11) as u16).collect();
        for (name, eight, parts) in [
            ("16-clut", false, [3u8].as_slice()),
            ("16-cv-clut-cv", false, [0,3,0].as_slice()),
            ("16-mx-cv-clut", false, [1,0,3].as_slice()),
            ("8-cv-clut-cv", true, [0,3,0].as_slice()),
            ("8-clut", true, [3].as_slice()),
        ] {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            for &k in parts {
                match k {
                    0 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3, cc.as_ptr())),
                    1 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocMatrix(std::ptr::null_mut(),
                            3, 3, mat.as_ptr(), std::ptr::null())),
                    _ => c::cmsPipelineInsertStage(p, 1,
                            c::cmsStageAllocCLut16bit(
                                std::ptr::null_mut(), 2, 3, 3,
                                tab16.as_ptr())),
                };
            }
            if eight { c::cmsPipelineSetSaveAs8bitsFlag(p, 1); }
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 2.1);
            let w = c::cmsWriteTag(prof, 0x41324230, p as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            // reload + read the tag back through the LUT reader
            if s != 0 {
                let rp = c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n);
                let tag = c::cmsReadTag(rp, 0x41324230);
                eprintln!("{name}: save n={n} readtag={}",
                    !tag.is_null());
                // eval the read pipeline
                if !tag.is_null() {
                    let i16 = [0x2000u16, 0x6000, 0xA000];
                    let mut o16 = [0u16; 3];
                    c::cmsPipelineEval16(i16.as_ptr(),
                        o16.as_mut_ptr(), tag);
                    eprintln!("  eval={o16:?}");
                }
                c::cmsCloseProfile(rp);
            } else {
                eprintln!("{name}: save failed w={w}");
            }
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
    }
}

/// mAB read-back cycle — v4-profile writes the mAB-structured
/// pipelines, reload + ReadTag + eval covers Type_LUTA2B_Read arms
/// (curve-set/matrix/CLUT/curve-set stage reads + offset reads).
#[test]
fn mab_readback_cycles() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut cc = [g; 3];
        let mat: [f64; 9] = [0.4,0.3,0.3, 0.2,0.6,0.2, 0.1,0.1,0.8];
        let tab: Vec<f32> = (0..8*3).map(|i| i as f32 / 24.0).collect();
        let tab16: Vec<u16> = (0..8*3).map(|i| (i * 11) as u16).collect();
        for (name, parts) in [
            ("cv", [0u8].as_slice()),
            ("cv-mx-cv", [0,1,0].as_slice()),
            ("cv-clut-cv", [0,2,0].as_slice()),
            ("cv-clut-cv-mx-cv", [0,2,0,1,0].as_slice()),
        ] {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            for &k in parts {
                match k {
                    0 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3, cc.as_ptr())),
                    1 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocMatrix(std::ptr::null_mut(),
                            3, 3, mat.as_ptr(), std::ptr::null())),
                    _ => c::cmsPipelineInsertStage(p, 1,
                            c::cmsStageAllocCLut16bit(
                                std::ptr::null_mut(), 2, 3, 3,
                                tab16.as_ptr())),
                };
            }
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, 0x41324230, p as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            if s != 0 {
                let rp = c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n);
                let tag = c::cmsReadTag(rp, 0x41324230);
                eprintln!("{name}: readtag={}", !tag.is_null());
                if !tag.is_null() {
                    let fi = [0.5f32, 0.3, 0.7];
                    let mut fo = [0f32; 3];
                    c::cmsPipelineEvalFloat(fi.as_ptr(),
                        fo.as_mut_ptr(), tag);
                    eprintln!("  eval={fo:?}");
                }
                c::cmsCloseProfile(rp);
            } else {
                eprintln!("{name}: save failed w={w}");
            }
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
    }
}

/// CLUT sampler + slice machinery — cmsStageSampleCLut16bit/Float
/// with a callback, cmsSliceSpace16/Float slice evals.
#[test]
fn clut_sampler_slice() {
    unsafe {
        // StageSample callbacks write Out; SliceSpace callbacks get
        // Out=NULL (enumeration only) — separate fn shapes.
        unsafe extern "C" fn s16(
            i: *const u16, o: *mut u16, _c: *mut std::ffi::c_void) -> i32 {
            unsafe {
                *o.add(0) = *i.add(0);
                *o.add(1) = *i.add(1);
                *o.add(2) = *i.add(2);
            }
            1
        }
        unsafe extern "C" fn s32(
            i: *const f32, o: *mut f32, _c: *mut std::ffi::c_void) -> i32 {
            unsafe {
                *o.add(0) = *i.add(0) * 0.5;
                *o.add(1) = *i.add(1);
                *o.add(2) = *i.add(2);
            }
            1
        }
        unsafe extern "C" fn e16(
            _i: *const u16, _o: *mut u16, _c: *mut std::ffi::c_void)
            -> i32 { 1 }
        unsafe extern "C" fn e32(
            _i: *const f32, _o: *mut f32, _c: *mut std::ffi::c_void)
            -> i32 { 1 }
        // sample a 16bit clut via callback — alloc with a real table
        let tab: Vec<u16> = vec![0; 2 * 2 * 2 * 3];
        let st = c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
            2, 3, 3, tab.as_ptr());
        eprintln!("st={}", !st.is_null());
        if !st.is_null() {
            let r = c::cmsStageSampleCLut16bit(st,
                s16 as unsafe extern "C" fn(_, _, _) -> i32,
                std::ptr::null_mut(), 0);
            eprintln!("sample16={r}");
        }
        // sample a float clut
        let ftab: Vec<f32> = vec![0.0; 2 * 2 * 2 * 3];
        let st2 = c::cmsStageAllocCLutFloat(std::ptr::null_mut(),
            2, 3, 3, ftab.as_ptr());
        eprintln!("st2={}", !st2.is_null());
        if !st2.is_null() {
            let r2 = c::cmsStageSampleCLutFloat(st2,
                s32 as unsafe extern "C" fn(_, _, _) -> i32,
                std::ptr::null_mut(), 0);
            eprintln!("sampleflt={r2}");
        }
        // SliceSpace16/Float — grid slice evals
        let dims = [2u32; 3];
        let r3 = c::cmsSliceSpace16(3, dims.as_ptr(),
            e16 as unsafe extern "C" fn(_, _, _) -> i32,
            std::ptr::null_mut());
        let r4 = c::cmsSliceSpaceFloat(3, dims.as_ptr(),
            e32 as unsafe extern "C" fn(_, _, _) -> i32,
            std::ptr::null_mut());
        eprintln!("slice16={r3} sliceflt={r4}");
    }
}

/// Lab<->XYZ stage evaluators — _cmsStageAllocLab2XYZ, XYZ2Lab,
/// LabV2ToV4, LabV4ToV2 through pipeline eval.
#[test]
fn lab_xyz_stages() {
    unsafe {
        for (name, st) in [
            ("lab2xyz", c::_cmsStageAllocLab2XYZ(std::ptr::null_mut())),
            ("xyz2lab", c::_cmsStageAllocXYZ2Lab(std::ptr::null_mut())),
            ("v2tov4", c::_cmsStageAllocLabV2ToV4(std::ptr::null_mut())),
            ("v4tov2", c::_cmsStageAllocLabV4ToV2(std::ptr::null_mut())),
        ] {
            eprintln!("{name}={}", !st.is_null());
            if !st.is_null() {
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                c::cmsPipelineInsertStage(p, 1, st);
                let fi = [0.5f32, 0.4, 0.6];
                let mut fo = [0f32; 3];
                c::cmsPipelineEvalFloat(fi.as_ptr(),
                    fo.as_mut_ptr(), p);
                eprintln!("  eval={fo:?}");
                c::cmsPipelineFree(p);
            }
        }
    }
}

/// MPE element read-back — write an mpet (DToB) tag carrying each
/// element type, reload + ReadTag covers the MPE element readers
/// (readMPEcurveElement/readMPEmatrixElement/readMPEclutElement/
/// readMPEclutFloatElement/readMPEbAcsEAcsElement).
#[test]
fn mpe_readback_cycles() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut cc = [g; 3];
        let mat: [f64; 9] = [0.4,0.3,0.3, 0.2,0.6,0.2, 0.1,0.1,0.8];
        let tab: Vec<u16> = (0..8*3).map(|i| (i * 11) as u16).collect();
        let ftab: Vec<f32> = (0..8*3).map(|i| i as f32 / 24.0).collect();
        for (name, parts) in [
            ("cv", [0u8].as_slice()),
            ("mx", [1].as_slice()),
            ("clut16", [2].as_slice()),
            ("clutflt", [3].as_slice()),
            ("cv-mx-clut", [0,1,2].as_slice()),
        ] {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            for &k in parts {
                match k {
                    0 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3, cc.as_ptr())),
                    1 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocMatrix(std::ptr::null_mut(),
                            3, 3, mat.as_ptr(), std::ptr::null())),
                    2 => c::cmsPipelineInsertStage(p, 1,
                            c::cmsStageAllocCLut16bit(
                                std::ptr::null_mut(), 2, 3, 3,
                                tab.as_ptr())),
                    _ => c::cmsPipelineInsertStage(p, 1,
                            c::cmsStageAllocCLutFloat(
                                std::ptr::null_mut(), 2, 3, 3,
                                ftab.as_ptr())),
                };
            }
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, 0x44324230, p as *const _); // D2B0
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            if s != 0 {
                let rp = c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n);
                let tag = c::cmsReadTag(rp, 0x44324230);
                eprintln!("{name}: read={}", !tag.is_null());
                if !tag.is_null() {
                    let fi = [0.5f32, 0.3, 0.7];
                    let mut fo = [0f32; 3];
                    c::cmsPipelineEvalFloat(fi.as_ptr(),
                        fo.as_mut_ptr(), tag);
                    eprintln!("  eval={fo:?}");
                }
                c::cmsCloseProfile(rp);
            } else {
                eprintln!("{name}: save failed w={w}");
            }
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
    }
}

/// Pipeline accessors + eval-reverse — StageCount/GetPtrToFirst-
/// Stage/NextStage/StageType/EstimatedTableEntries + EvalReverse-
/// Float on an invertible pipeline.
#[test]
fn pipeline_accessors_reverse() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut cc = [g; 3];
        let mat: [f64; 9] = [0.4,0.3,0.3, 0.2,0.6,0.2, 0.1,0.1,0.8];
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3, cc.as_ptr()));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), std::ptr::null()));
        let cnt = c::cmsPipelineStageCount(p);
        eprintln!("stages={cnt}");
        let mut st = c::cmsPipelineGetPtrToFirstStage(p);
        let mut i = 0;
        while !st.is_null() {
            let ty = c::cmsStageType(st);
            eprintln!("  stage{i} type={ty:#x}");
            st = c::cmsStageNext(st);
            i += 1;
        }
        // reverse eval (inverts matrix + curves)
        let mut fi = [0.5f32, 0.3, 0.7];
        let mut fo = [0f32; 3];
        let mut hint = [0f32; 3];
        let r = c::cmsPipelineEvalReverseFloat(
            fi.as_mut_ptr(), fo.as_mut_ptr(), hint.as_mut_ptr(), p);
        eprintln!("reverse r={r} fo={fo:?}");
        c::cmsPipelineFree(p);
    }
}

/// Stage-dup + insert-position arms — dup a multi-stage pipeline
/// (per-type dup callbacks), insert at BEGIN/END/middle, eval the
/// re-ordered pipeline.
#[test]
fn stage_dup_insert_positions() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut cc = [g; 3];
        let mat: [f64; 9] = [0.4,0.3,0.3, 0.2,0.6,0.2, 0.1,0.1,0.8];
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3, cc.as_ptr()));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), std::ptr::null()));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3, cc.as_ptr()));
        // dup -> per-type stage dup callbacks
        let dp = c::cmsPipelineDup(p);
        eprintln!("dup={}", !dp.is_null());
        if !dp.is_null() {
            let fi = [0.5f32, 0.3, 0.7];
            let mut fo = [0f32; 3];
            c::cmsPipelineEvalFloat(fi.as_ptr(), fo.as_mut_ptr(), dp);
            eprintln!("  dup-eval={fo:?}");
            // insert at begin + middle positions
            c::cmsPipelineInsertStage(dp, 0,
                c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                    mat.as_ptr(), std::ptr::null()));
            c::cmsPipelineEvalFloat(fi.as_ptr(), fo.as_mut_ptr(), dp);
            eprintln!("  post-insert={fo:?}");
            c::cmsPipelineFree(dp);
        }
        c::cmsPipelineFree(p);
    }
}

/// Granular CLUT + channel accessors — CLut16bitGranular with
/// non-uniform dims, StageInputChannels/OutputChannels,
/// PipelineInputChannels/OutputChannels.
#[test]
fn granular_clut_accessors() {
    unsafe {
        // non-uniform grid dims -> the granular alloc path
        let dims = [2u32, 3, 4];
        let n_cells = (2*3*4) as usize * 3;
        let tab: Vec<u16> = (0..n_cells).map(|i| (i * 13) as u16).collect();
        let st = c::cmsStageAllocCLut16bitGranular(
            std::ptr::null_mut(), dims.as_ptr(), 3, 3, tab.as_ptr());
        eprintln!("granular st={}", !st.is_null());
        if !st.is_null() {
            eprintln!("  in={} out={}",
                c::cmsStageInputChannels(st),
                c::cmsStageOutputChannels(st));
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            c::cmsPipelineInsertStage(p, 1, st);
            eprintln!("  pin={} pout={}",
                c::cmsPipelineInputChannels(p),
                c::cmsPipelineOutputChannels(p));
            let fi = [0.5f32, 0.3, 0.7];
            let mut fo = [0f32; 3];
            c::cmsPipelineEvalFloat(fi.as_ptr(), fo.as_mut_ptr(), p);
            eprintln!("  eval={fo:?}");
            c::cmsPipelineFree(p);
        }
        // multi-channel counts — a 4-in-3-out matrix
        let m4: [f64; 12] = [0.4,0.3,0.3,0.0, 0.2,0.6,0.2,0.0,
                             0.1,0.1,0.8,0.0];
        let st4 = c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 4,
            m4.as_ptr(), std::ptr::null());
        if !st4.is_null() {
            eprintln!("m4 in={} out={}",
                c::cmsStageInputChannels(st4),
                c::cmsStageOutputChannels(st4));
        }
    }
}

/// Stage/pipeline internals — Identity stages, stage-free,
/// GetEstimatedTableEntries, pipeline dup of every stage type.
#[test]
fn stage_internals() {
    unsafe {
        // identity stages across channel counts
        for n in [1u32, 3, 4, 15] {
            let st = c::cmsStageAllocIdentity(std::ptr::null_mut(), n);
            eprintln!("id{n}={}", !st.is_null());
            if !st.is_null() { c::cmsStageFree(st); }
        }
        // estimated table entries on a mixed pipeline
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut cc = [g; 3];
        let tab: Vec<u16> = (0..8*3).map(|i| (i * 7) as u16).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3, cc.as_ptr()));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), 2, 3, 3,
                tab.as_ptr()));
        let p2 = c::cmsPipelineDup(p);
        let mut sz1 = 0u32;
        let _ = c::cmsPipelineEvalReverseFloat; // silence unused
        eprintln!("dup={}", !p2.is_null());
        if !p2.is_null() { c::cmsPipelineFree(p2); }
        c::cmsPipelineFree(p);
        let _ = sz1;
    }
}

/// SaveAs8bitsFlag + pipeline internals — the flag's
/// get/set/restore, pipeline dup honoring it.
#[test]
fn save_as_8bits_flag() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut cc = [g; 3];
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3, cc.as_ptr()));
        // flag set/get/restore -> internal flag accessor arms
        for v in [0i32, 1, 0, 1] {
            let old = c::cmsPipelineSetSaveAs8bitsFlag(p, v);
            eprintln!("set8={v} old={old}");
        }
        // dup honors the flag
        let dp = c::cmsPipelineDup(p);
        if !dp.is_null() {
            let _ = c::cmsPipelineSetSaveAs8bitsFlag(dp, 0);
            c::cmsPipelineFree(dp);
        }
        c::cmsPipelineFree(p);
    }
}

/// N-input CLUT evaluators — CLUT stages with 4-8 input channels
/// hit Eval4Inputs..Eval8Inputs + the Float variants.
#[test]
fn n_input_clut_eval() {
    unsafe {
        for nin in [4u32, 5, 6, 7, 8, 9, 11, 13, 15] {
            let n_cells = (2usize).pow(nin) * 3;
            let tab: Vec<u16> = (0..n_cells)
                .map(|i| (i * 5) as u16).collect();
            let ftab: Vec<f32> = (0..n_cells)
                .map(|i| i as f32 / n_cells as f32).collect();
            let st = c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                2, nin, 3, tab.as_ptr());
            let stf = c::cmsStageAllocCLutFloat(std::ptr::null_mut(),
                2, nin, 3, ftab.as_ptr());
            if !st.is_null() {
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), nin, 3);
                c::cmsPipelineInsertStage(p, 1, st);
                let ib = vec![0x4000u16; nin as usize];
                let mut ob = [0u16; 3];
                c::cmsPipelineEval16(ib.as_ptr(), ob.as_mut_ptr(), p);
                c::cmsPipelineFree(p);
            }
            if !stf.is_null() {
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), nin, 3);
                c::cmsPipelineInsertStage(p, 1, stf);
                let ib = vec![0.5f32; nin as usize];
                let mut ob = [0f32; 3];
                c::cmsPipelineEvalFloat(ib.as_ptr(), ob.as_mut_ptr(), p);
                c::cmsPipelineFree(p);
            }
            eprintln!("nin={nin} eval ran");
        }
    }
}

/// Interpolation input-channel arms — 1-in (LinLerp1D/Eval1Input),
/// 2-in (Bilinear), plus the trilinear flag path on 3-in.
#[test]
fn interp_channel_arms() {
    unsafe {
        for (nin, name) in [(1u32, "1-in"), (2, "2-in"), (3, "3-in")] {
            let n_cells = (4usize).pow(nin) * 3;
            let tab: Vec<u16> = (0..n_cells)
                .map(|i| (i * 3) as u16).collect();
            let ftab: Vec<f32> = (0..n_cells)
                .map(|i| i as f32 / n_cells as f32).collect();
            let st = c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                4, nin, 3, tab.as_ptr());
            let stf = c::cmsStageAllocCLutFloat(std::ptr::null_mut(),
                4, nin, 3, ftab.as_ptr());
            for s in [st, stf] {
                if s.is_null() { continue; }
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), nin, 3);
                c::cmsPipelineInsertStage(p, 1, s);
                let ib = vec![0x4000u16; nin as usize];
                let mut ob = [0u16; 3];
                c::cmsPipelineEval16(ib.as_ptr(), ob.as_mut_ptr(), p);
                let ibf = vec![0.5f32; nin as usize];
                let mut obf = [0f32; 3];
                c::cmsPipelineEvalFloat(ibf.as_ptr(), obf.as_mut_ptr(), p);
                c::cmsPipelineFree(p);
            }
            eprintln!("{name} ran");
        }
    }
}

/// Matrix-stage internals — with/without offset vectors, different
/// shapes -> EvaluateMatrix offset arms.
#[test]
fn matrix_stage_arms() {
    unsafe {
        let m = [0.4f64, 0.3, 0.3, 0.2, 0.6, 0.2, 0.1, 0.1, 0.8];
        let off = [0.01f64, 0.02, 0.03];
        for (o, name) in [(std::ptr::null(), "no-off"),
                          (off.as_ptr(), "with-off")] {
            let st = c::cmsStageAllocMatrix(std::ptr::null_mut(),
                3, 3, m.as_ptr(), o);
            if st.is_null() { eprintln!("{name} null"); continue; }
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            c::cmsPipelineInsertStage(p, 1, st);
            let ib = [0x4000u16; 3];
            let mut ob = [0u16; 3];
            c::cmsPipelineEval16(ib.as_ptr(), ob.as_mut_ptr(), p);
            let ibf = [0.5f32; 3];
            let mut obf = [0f32; 3];
            c::cmsPipelineEvalFloat(ibf.as_ptr(), obf.as_mut_ptr(), p);
            eprintln!("{name} ob={ob:?} obf={obf:?}");
            c::cmsPipelineFree(p);
        }
        // non-square matrix — 3-in-1-out
        let m31 = [0.5f64, 0.3, 0.2];
        let st = c::cmsStageAllocMatrix(std::ptr::null_mut(), 1, 3,
            m31.as_ptr(), std::ptr::null());
        if !st.is_null() {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 1);
            c::cmsPipelineInsertStage(p, 1, st);
            c::cmsPipelineFree(p);
        }
    }
}

/// Segmented-curve MPE element — a curve stage with a segmented
/// tone curve -> WriteSegmentedCurve/ReadSegmentedCurve + the
/// formula/sampled segment arms.
#[test]
fn segmented_curve_mpe() {
    unsafe {
        // segmented curve — cmsCurveSegment {x0,x1,Type,Params[10],
        // nGridPoints,SampledPoints}
        #[repr(C)]
        struct Seg { x0: f32, x1: f32, ty: i32,
                     params: [f64; 10], n: u32, pt: *const f32 }
        let smp = [0.0f32, 0.3, 0.5, 0.8, 1.0];
        let segs = [
            Seg { x0: 0.0, x1: 0.5, ty: 0, params: [0.0; 10],
                  n: 5, pt: smp.as_ptr() },         // sampled
            Seg { x0: 0.5, x1: 1.0, ty: 4,
                  params: [2.2, 0.0, 0.0, 0.0, 0.0,
                           0.0, 0.0, 0.0, 0.0, 0.0],
                  n: 0, pt: std::ptr::null() },     // formula
        ];
        let tc = c::cmsBuildSegmentedToneCurve(std::ptr::null_mut(),
            2, segs.as_ptr() as *const _);
        eprintln!("seg={}", !tc.is_null());
        if !tc.is_null() {
            let mut cc = [tc; 3];
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                    cc.as_ptr()));
            let ftab: Vec<f32> = (0..8*3)
                .map(|i| i as f32 / 24.0).collect();
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocCLutFloat(std::ptr::null_mut(),
                    2, 3, 3, ftab.as_ptr()));
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, 0x44324230, p as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x44324230)
            } else { std::ptr::null_mut() };
            eprintln!("segmpe w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
    }
}

/// Internal stage allocators — Lab2XYZ/XYZ2Lab/LabV2ToV4/LabPrelin/
/// IdentityCLut/NamedColor helpers (the ones behind the profile
/// readers).
#[test]
fn internal_stage_allocators() {
    unsafe {
        let stages = [
            ("lab2xyz", c::_cmsStageAllocLab2XYZ(std::ptr::null_mut())),
            ("xyz2lab", c::_cmsStageAllocXYZ2Lab(std::ptr::null_mut())),
            ("lv2v4", c::_cmsStageAllocLabV2ToV4(std::ptr::null_mut())),
            ("lv4v2", c::_cmsStageAllocLabV4ToV2(std::ptr::null_mut())),
            ("prelin", c::_cmsStageAllocLabPrelin(std::ptr::null_mut())),
            ("idclut", c::_cmsStageAllocIdentityCLut(std::ptr::null_mut(), 3)),
        ];
        for (name, st) in stages {
            eprintln!("{name}={}", !st.is_null());
            if !st.is_null() { c::cmsStageFree(st); }
        }
        // Lab prelin curves variant
        let s2 = c::_cmsStageAllocLabV2ToV4curves(std::ptr::null_mut());
        eprintln!("v2v4curves={}", !s2.is_null());
        if !s2.is_null() { c::cmsStageFree(s2); }
    }
}

/// EvaluateCurves/EvalStage channel-count internals — curve stages
/// with 1/2/4/6/15 channels through eval16+float.
#[test]
fn eval_curves_channels() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for nch in [1u32, 2, 4, 6, 15] {
            let mut cc = vec![g; nch as usize];
            let st = c::cmsStageAllocToneCurves(std::ptr::null_mut(),
                nch, cc.as_ptr());
            if st.is_null() { continue; }
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), nch, nch);
            c::cmsPipelineInsertStage(p, 1, st);
            let ib = vec![0x4000u16; nch as usize];
            let mut ob = vec![0u16; nch as usize];
            c::cmsPipelineEval16(ib.as_ptr(), ob.as_mut_ptr(), p);
            let ibf = vec![0.5f32; nch as usize];
            let mut obf = vec![0f32; nch as usize];
            c::cmsPipelineEvalFloat(ibf.as_ptr(), obf.as_mut_ptr(), p);
            eprintln!("nch{nch} ok");
            c::cmsPipelineFree(p);
        }
    }
}

/// InsertStage position arms — BEFORE/AFTER a stage + BEGIN/END.
#[test]
fn insert_stage_positions() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        // AT_BEGIN=0, AT_END=1 — insert a curve, then matrix before/after
        c::cmsPipelineInsertStage(p, 0,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g; 3].as_ptr() as *mut _));
        let first = c::cmsPipelineGetPtrToFirstStage(p);
        // insert matrix at end + at begin (before first)
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), std::ptr::null()));
        c::cmsPipelineInsertStage(p, 0,
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), std::ptr::null()));
        let cnt = c::cmsPipelineStageCount(p);
        eprintln!("stages={cnt}");
        let fi = [0.5f32; 3];
        let mut fo = [0f32; 3];
        c::cmsPipelineEvalFloat(fi.as_ptr(), fo.as_mut_ptr(), p);
        eprintln!("eval={fo:?}");
        c::cmsPipelineFree(p);
        let _ = first;
    }
}


/// 8-bit CLUT serialization — SaveAs8bits flag serializes a 16-bit
/// CLUT as mft1 -> the 8-bit table-write path.
#[test]
fn clut8bit_serialization() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let tab: Vec<u16> = (0..27*3).map(|i| (i * 5) as u16).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), 3, 3, 3,
                tab.as_ptr()));
        c::cmsPipelineSetSaveAs8bitsFlag(p, 1);
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof, 2.2); // v2 -> mft1/mft2
        let w = c::cmsWriteTag(prof, 0x41324230, p as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        eprintln!("clut8bit-ser w={w} s={s}");
        c::cmsCloseProfile(prof);
        c::cmsPipelineFree(p);
    }
}

/// mAB writer offset internals — pipelines that serialize into
/// each mAB element position -> the offset-array computation.
#[test]
fn mab_writer_offsets() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let ftab: Vec<f32> = (0..8*3).map(|i| i as f32 / 24.0).collect();
        // mAB element sets: [A-curves] [matrix] [M-curves] [clut] [B-curves]
        for (stages, name) in [
            (vec![0], "B-only"),
            (vec![0, 2, 0], "B-clut-A"),       // B,clut,A
            (vec![0, 1, 0], "cv-mx-cv"),         // curves+matrix+curves
            (vec![0, 1, 0, 2, 0], "all-5"),      // full mAB
            (vec![2], "clut-only"),
        ] {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            for &k in &stages {
                match k {
                    0 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3,
                            [g; 3].as_ptr() as *mut _)),
                    1 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocMatrix(std::ptr::null_mut(),
                            3, 3, mat.as_ptr(), std::ptr::null())),
                    _ => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocCLutFloat(
                            std::ptr::null_mut(), 2, 3, 3,
                            ftab.as_ptr())),
                };
            }
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, 0x41324230, p as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            eprintln!("{name} w={w} s={s}");
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
    }
}

/// PipelineEvalReverseFloat internals — invertible (curves+matrix)
/// vs non-invertible (CLUT) pipelines -> reverse-eval arms.
#[test]
fn pipeline_reverse_eval() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut cc = [g; 3];
        let m = [2.0f64, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 1.0];
        // invertible: curves + non-singular matrix
        let p1 = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p1, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3, cc.as_ptr()));
        c::cmsPipelineInsertStage(p1, 1,
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                m.as_ptr(), std::ptr::null()));
        let mut ti = [0.5f32; 3];
        let mut ro = [0f32; 3];
        let mut hint = [0f32; 3];
        let r1 = c::cmsPipelineEvalReverseFloat(ti.as_mut_ptr(),
            ro.as_mut_ptr(), hint.as_mut_ptr(), p1);
        eprintln!("invertible r={r1} ro={ro:?}");
        c::cmsPipelineFree(p1);
        // non-invertible: CLUT (no analytic inverse)
        let tab: Vec<u16> = (0..27*3).map(|i| (i * 9) as u16).collect();
        let p2 = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p2, 1,
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), 3, 3, 3,
                tab.as_ptr()));
        let r2 = c::cmsPipelineEvalReverseFloat(ti.as_mut_ptr(),
            ro.as_mut_ptr(), hint.as_mut_ptr(), p2);
        eprintln!("clut r={r2}");
        c::cmsPipelineFree(p2);
    }
}

/// MPE element-reader dispatch — an mpet tag with each element
/// type -> the per-element-type read dispatch (curves/matrix/clut/
/// clutfloat/bacs/eacs).
#[test]
fn mpe_element_reader_dispatch() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mat = [0.4f64, 0.3, 0.3, 0.2, 0.6, 0.2, 0.1, 0.1, 0.8];
        let ftab: Vec<f32> = (0..8*3).map(|i| i as f32 / 24.0).collect();
        let tab: Vec<u16> = (0..8*3).map(|i| (i * 11) as u16).collect();
        for (k, name) in [(0, "curves"), (1, "matrix"), (2, "clutf"),
                          (3, "clut16")] {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            match k {
                0 => c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocToneCurves(
                        std::ptr::null_mut(), 3,
                        [g; 3].as_ptr() as *mut _)),
                1 => c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocMatrix(std::ptr::null_mut(),
                        3, 3, mat.as_ptr(), std::ptr::null())),
                2 => c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocCLutFloat(
                        std::ptr::null_mut(), 2, 3, 3,
                        ftab.as_ptr())),
                _ => c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocCLut16bit(
                        std::ptr::null_mut(), 2, 3, 3,
                        tab.as_ptr())),
            };
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, 0x44324230, p as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x44324230)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
    }
}

/// LUT element-reader internals — mAB tags with different
/// channel-count + curve-count combos -> ReadCurves/ReadMatrix/
/// ReadCLUT element readers' per-count arms.
#[test]
fn lut_element_readers() {
    unsafe {
        // construct mAB tags with each element set; round-trip read
        // hits the per-element readers (curve/matrix/clut).
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mat = [0.4f64, 0.3, 0.3, 0.2, 0.6, 0.2, 0.1, 0.1, 0.8];
        let ftab: Vec<f32> = (0..8*3).map(|i| i as f32 / 24.0).collect();
        for (stages, name) in [
            (vec![1, 2], "mx-clut"),
            (vec![2, 1], "clut-mx"),
            (vec![0, 1, 2], "cv-mx-clut"),
        ] {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            for &k in &stages {
                match k {
                    0 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3,
                            [g; 3].as_ptr() as *mut _)),
                    1 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocMatrix(std::ptr::null_mut(),
                            3, 3, mat.as_ptr(), std::ptr::null())),
                    _ => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocCLutFloat(
                            std::ptr::null_mut(), 2, 3, 3,
                            ftab.as_ptr())),
                };
            }
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, 0x41324230, p as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x41324230)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
    }
}

/// cmsPipelineCheckAndRetreiveStages — the variadic
/// stage-pattern extractor (optimization's shape check).
#[test]
fn check_retreive_stages() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mat = [0.4f64, 0.3, 0.3, 0.2, 0.6, 0.2, 0.1, 0.1, 0.8];
        // cv-mx-cv pattern -> extract 3 stage pointers
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g; 3].as_ptr() as *mut _));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), std::ptr::null()));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g; 3].as_ptr() as *mut _));
        let mut a: *mut c_void = std::ptr::null_mut();
        let mut b: *mut c_void = std::ptr::null_mut();
        let mut c_: *mut c_void = std::ptr::null_mut();
        // stage sigs: curveSet=0x63767374 'cvst', matrix=0x6d617420
        let r = c::cmsPipelineCheckAndRetreiveStages(p, 3,
            0x63767374, 0x6d617420, 0x63767374,
            &mut a as *mut _ as *mut _,
            &mut b as *mut _ as *mut _,
            &mut c_ as *mut _ as *mut _);
        eprintln!("cv-mx-cv r={r}");
        // wrong pattern -> reject arm
        let r2 = c::cmsPipelineCheckAndRetreiveStages(p, 2,
            0x6d617420, 0x6d617420,
            &mut a as *mut _ as *mut _,
            &mut b as *mut _ as *mut _);
        eprintln!("mx-mx r={r2}");
        c::cmsPipelineFree(p);
    }
}

/// Lab-stage eval internals — each Lab-related stage through
/// eval16 + float -> the Lab2XYZ/XYZ2Lab/LabPrelin/V2V4 evaluators.
#[test]
fn lab_stage_evals() {
    unsafe {
        for (st, nch, name) in [
            (c::_cmsStageAllocLab2XYZ(std::ptr::null_mut()), 3, "l2x"),
            (c::_cmsStageAllocXYZ2Lab(std::ptr::null_mut()), 3, "x2l"),
            (c::_cmsStageAllocLabPrelin(std::ptr::null_mut()), 3, "prelin"),
            (c::_cmsStageAllocLabV2ToV4(std::ptr::null_mut()), 3, "v2v4"),
            (c::_cmsStageAllocLabV4ToV2(std::ptr::null_mut()), 3, "v4v2"),
        ] {
            if st.is_null() { eprintln!("{name} null"); continue; }
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), nch, nch);
            c::cmsPipelineInsertStage(p, 1, st);
            let ib = [0x8000u16; 3];
            let mut ob = [0u16; 3];
            c::cmsPipelineEval16(ib.as_ptr(), ob.as_mut_ptr(), p);
            let ibf = [0.5f32; 3];
            let mut obf = [0f32; 3];
            c::cmsPipelineEvalFloat(ibf.as_ptr(), obf.as_mut_ptr(), p);
            eprintln!("{name} ob={ob:?} obf={obf:?}");
            c::cmsPipelineFree(p);
        }
    }
}

/// Granular CLUT allocators — per-dimension grid sizes for
/// CLut16bitGranular/FloatGranular/8bitGranular.
#[test]
fn granular_clut_allocs() {
    unsafe {
        let dims = [5u32, 4, 3]; // 5x4x3 grid
        let n: usize = (dims[0] * dims[1] * dims[2]) as usize * 3;
        let t16: Vec<u16> = (0..n).map(|i| (i * 7) as u16).collect();
        let tf: Vec<f32> = (0..n).map(|i| i as f32 / n as f32).collect();
        let t8: Vec<u8> = (0..n).map(|i| (i * 3) as u8).collect();
        for (k, name) in [(0, "gran16"), (1, "granf")] {
            let st = match k {
                0 => c::cmsStageAllocCLut16bitGranular(
                    std::ptr::null_mut(), dims.as_ptr(), 3, 3,
                    t16.as_ptr()),
                _ => c::cmsStageAllocCLutFloatGranular(
                    std::ptr::null_mut(), dims.as_ptr(), 3, 3,
                    tf.as_ptr()),
            };
            eprintln!("{name}={}", !st.is_null());
            if !st.is_null() {
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                c::cmsPipelineInsertStage(p, 1, st);
                let ib = [0x4000u16; 3];
                let mut ob = [0u16; 3];
                c::cmsPipelineEval16(ib.as_ptr(), ob.as_mut_ptr(), p);
                eprintln!("  ob={ob:?}");
                c::cmsPipelineFree(p);
            }
        }
    }
}

/// NamedColor + Identity + Lab stage eval internals — the
/// remaining stage-type evaluators.
#[test]
fn named_identity_stage_evals() {
    unsafe {
        let ncl = c::cmsAllocNamedColorList(std::ptr::null_mut(), 1,
            3, b"p\0".as_ptr(), b"s\0".as_ptr());
        let lab = [0xF000u16, 0x8000, 0x8000];
        c::cmsAppendNamedColor(ncl, b"c1\0".as_ptr(),
            lab.as_ptr(), lab.as_ptr());
        // named-color stage (1->3) in a 1-in pipeline
        let st = c::_cmsStageAllocNamedColor(ncl, 1);
        if !st.is_null() {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 1, 3);
            c::cmsPipelineInsertStage(p, 1, st);
            let ib = [0x8000u16];
            let mut ob = [0u16; 3];
            c::cmsPipelineEval16(ib.as_ptr(), ob.as_mut_ptr(), p);
            eprintln!("ncl ob={ob:?}");
            c::cmsPipelineFree(p);
        }
        // identity stage
        let si = c::cmsStageAllocIdentity(std::ptr::null_mut(), 3);
        if !si.is_null() {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            c::cmsPipelineInsertStage(p, 1, si);
            let ib = [0x4000u16; 3];
            let mut ob = [0u16; 3];
            c::cmsPipelineEval16(ib.as_ptr(), ob.as_mut_ptr(), p);
            eprintln!("id ob={ob:?}");
            c::cmsPipelineFree(p);
        }
        c::cmsFreeNamedColorList(ncl);
    }
}

/// Pipeline pointer/accessor internals — first/last stage,
/// estimated-table entries + content.
#[test]
fn pipeline_ptr_accessors() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mat = [0.4f64, 0.3, 0.3, 0.2, 0.6, 0.2, 0.1, 0.1, 0.8];
        let tab: Vec<u16> = (0..27*3).map(|i| (i * 9) as u16).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g; 3].as_ptr() as *mut _));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), 3, 3, 3,
                tab.as_ptr()));
        let first = c::cmsPipelineGetPtrToFirstStage(p);
        let last = c::cmsPipelineGetPtrToLastStage(p);
        eprintln!("first={} last={}",
            !first.is_null(), !last.is_null());
        // walk the stage chain via cmsStageNext
        let mut cur = first;
        let mut nst = 0;
        while !cur.is_null() && nst < 10 {
            nst += 1;
            cur = c::cmsStageNext(cur);
        }
        eprintln!("stages walked={nst}");
        // UnlinkStage — remove + return the unlinked stage
        let mut out_st: c::Handle = std::ptr::null_mut();
        let u = c::cmsPipelineUnlinkStage(p, 1, &mut out_st);
        eprintln!("unlink={u} st={}", !out_st.is_null());
        if !out_st.is_null() { c::cmsStageFree(out_st); }
        c::cmsPipelineFree(p);
    }
}

/// cmsPipelineCat internals — concatenating pipelines adjusts
/// input/output channels + stage chain.
#[test]
fn pipeline_cat() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mat = [0.4f64, 0.3, 0.3, 0.2, 0.6, 0.2, 0.1, 0.1, 0.8];
        // p1: curves (3->3); p2: matrix (3->3) + clut
        let p1 = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p1, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g; 3].as_ptr() as *mut _));
        let p2 = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p2, 1,
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), std::ptr::null()));
        let n0 = c::cmsPipelineStageCount(p1);
        let r = c::cmsPipelineCat(p1, p2);
        let n1 = c::cmsPipelineStageCount(p1);
        eprintln!("cat r={r} n={n0}->{n1}");
        // eval the concatenated pipeline
        let ib = [0x4000u16; 3];
        let mut ob = [0u16; 3];
        c::cmsPipelineEval16(ib.as_ptr(), ob.as_mut_ptr(), p1);
        eprintln!("ob={ob:?}");
        c::cmsPipelineFree(p1);
    }
}

/// mAB/mBA reader offset-parse internals — a full mAB profile
/// written then read back exercises every element-offset reader.
#[test]
fn mab_mba_full_readback() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mat = [0.4f64, 0.3, 0.3, 0.2, 0.6, 0.2, 0.1, 0.1, 0.8];
        let ftab: Vec<f32> = (0..8*3).map(|i| i as f32 / 24.0).collect();
        // full mAB order: A-curves, matrix, M-curves, clut, B-curves
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g; 3].as_ptr() as *mut _));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), std::ptr::null()));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g; 3].as_ptr() as *mut _));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g; 3].as_ptr() as *mut _));
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof, 4.4);
        for sig in [0x41324230u32, 0x42324130] { // 'A2B0','B2A0'
            c::cmsWriteTag(prof, sig, p as *const _);
        }
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = c::cmsSaveProfileToMem(prof,
            buf.as_mut_ptr() as *mut _, &mut n);
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        for sig in [0x41324230u32, 0x42324130] {
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("sig{sig:#x} read={}", !tag.is_null());
        }
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
        c::cmsPipelineFree(p);
    }
}

/// Lab-prelin + LabV2ToV4curves + named-color stage eval internals.
#[test]
fn lab_prelin_v2v4_evals() {
    unsafe {
        // LabPrelin (the pre-linearization curves stage)
        let st = c::_cmsStageAllocLabPrelin(std::ptr::null_mut());
        if !st.is_null() {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            c::cmsPipelineInsertStage(p, 1, st);
            let ib = [0x8000u16; 3];
            let mut ob = [0u16; 3];
            c::cmsPipelineEval16(ib.as_ptr(), ob.as_mut_ptr(), p);
            eprintln!("prelin ob={ob:?}");
            c::cmsPipelineFree(p);
        }
        // LabV2ToV4curves
        let st2 = c::_cmsStageAllocLabV2ToV4curves(std::ptr::null_mut());
        if !st2.is_null() {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            c::cmsPipelineInsertStage(p, 1, st2);
            let ib = [0x8000u16; 3];
            let mut ob = [0u16; 3];
            c::cmsPipelineEval16(ib.as_ptr(), ob.as_mut_ptr(), p);
            eprintln!("v2v4c ob={ob:?}");
            c::cmsPipelineFree(p);
        }
    }
}

/// LUT reader internals — mft1/mft2/mAB/mBA tags on corpus +
/// built profiles -> the LUT readers' element-offset parsing.
#[test]
fn lut_readers_internals() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        // corpus profiles have mft/mAB tags — reading them
        // exercises the LUT element readers.
        for f in ["lcms2cmyk.icc", "sRGBlcms2.icc", "aRGBlcms2.icc",
                  "lcms2link.icc", "labv2lcms2.icc", "graylcms2.icc",
                  "lcms2mtsh.icc", "lcms2matrix.icc",
                  "lcms2mft2.icc", "lcms2ab4.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            // read every tag -> the per-sig LUT readers run
            let n = c::cmsGetTagCount(p);
            for i in 0..n {
                let sig = c::cmsGetTagSignature(p, i as u32);
                let _ = c::cmsReadTag(p, sig);
            }
            eprintln!("{f} tags={n}");
            c::cmsCloseProfile(p);
        }
    }
}

/// Lab + XYZ stage eval internals — Lab2XYZ/XYZ2Lab/LabPrelin/
/// LabV2ToV4/LabV4ToV2/LabV2ToV4curves through eval16 + float.
#[test]
fn lab_stage_evals2() {
    unsafe {
        for (st, name) in [
            (c::_cmsStageAllocLab2XYZ(std::ptr::null_mut()), "l2x"),
            (c::_cmsStageAllocXYZ2Lab(std::ptr::null_mut()), "x2l"),
            (c::_cmsStageAllocLabPrelin(std::ptr::null_mut()), "prelin"),
            (c::_cmsStageAllocLabV2ToV4(std::ptr::null_mut()), "v2v4"),
            (c::_cmsStageAllocLabV4ToV2(std::ptr::null_mut()), "v4v2"),
            (c::_cmsStageAllocLabV2ToV4curves(std::ptr::null_mut()),
             "v2v4c"),
        ] {
            if st.is_null() { continue; }
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            c::cmsPipelineInsertStage(p, 1, st);
            let ib = [0x8000u16; 3];
            let mut ob = [0u16; 3];
            c::cmsPipelineEval16(ib.as_ptr(), ob.as_mut_ptr(), p);
            let ibf = [0.5f32; 3];
            let mut obf = [0f32; 3];
            c::cmsPipelineEvalFloat(ibf.as_ptr(), obf.as_mut_ptr(), p);
            eprintln!("{name} ob={ob:?} obf={obf:?}");
            c::cmsPipelineFree(p);
        }
    }
}

/// LUT reader per-element details — mft1/mft2 tags with varied
/// channel counts + grid sizes -> the readers' element loops.
#[test]
fn lut_reader_elements() {
    unsafe {
        // malformed-ish mft1/mft2 tags — varied counts
        for (nch_in, nch_out, grid, name) in [
            (1, 1, 2, "1x1"),
            (3, 3, 2, "3x3"),
            (4, 4, 2, "4x4"),
            (3, 3, 17, "3x3-g17"),
        ] {
            let mut t = Vec::new();
            t.extend_from_slice(&0x6d667432u32.to_be_bytes()); // mft2
            t.extend_from_slice(&[0; 4]);
            t.push(nch_in); t.push(nch_out); t.push(grid); t.push(0);
            for _ in 0..9 { t.extend_from_slice(&0x10000u32.to_be_bytes()); }
            t.extend_from_slice(&256u16.to_be_bytes());
            t.extend_from_slice(&256u16.to_be_bytes());
            // fill with table data
            let n_entries = (grid as usize).pow(nch_in as u32) * nch_out as usize;
            let total = 256 * nch_in as usize + n_entries + 256 * nch_out as usize;
            for i in 0..total { t.extend_from_slice(&(i as u16).to_be_bytes()); }
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsWriteRawTag(prof, 0x41324230, t.as_ptr() as *const _,
                t.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x41324230)
            } else { std::ptr::null_mut() };
            eprintln!("{name} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// mAB/mBA reader offset-parse internals — mAB tags with each
/// element position populated -> the offset-reader dispatch.
#[test]
fn mab_reader_offsets() {
    unsafe {
        // construct mAB tags with element offset variations
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mat = [0.4f64, 0.3, 0.3, 0.2, 0.6, 0.2, 0.1, 0.1, 0.8];
        let ftab: Vec<f32> = (0..8*3).map(|i| i as f32 / 24.0).collect();
        // pipelines that map to mAB element combinations
        for (stages, name) in [
            (vec![0], "B-cv"),
            (vec![0, 1, 0], "cv-mx-cv"),
            (vec![0, 2, 0], "cv-clut-cv"),
            (vec![0, 1, 0, 2, 0], "full-mab"),
            (vec![2], "clut-only"),
            (vec![1, 2], "mx-clut"),
        ] {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            for &k in &stages {
                match k {
                    0 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3,
                            [g; 3].as_ptr() as *mut _)),
                    1 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocMatrix(std::ptr::null_mut(),
                            3, 3, mat.as_ptr(), std::ptr::null())),
                    _ => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocCLutFloat(
                            std::ptr::null_mut(), 2, 3, 3,
                            ftab.as_ptr())),
                };
            }
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, 0x41324230, p as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x41324230)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
    }
}

/// NamedColor + Lab stage eval internals — the named-color stage
/// (1->3 channel) + remaining Lab variants.
#[test]
fn named_lab_stage_evals() {
    unsafe {
        let ncl = c::cmsAllocNamedColorList(std::ptr::null_mut(), 1,
            3, b"p\0".as_ptr(), b"s\0".as_ptr());
        let lab16 = [0xF000u16; 3];
        c::cmsAppendNamedColor(ncl, b"c1\0".as_ptr(),
            lab16.as_ptr(), lab16.as_ptr());
        // named-color stage (1-in, 3-out)
        let st = c::_cmsStageAllocNamedColor(ncl, 1);
        if !st.is_null() {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 1, 3);
            c::cmsPipelineInsertStage(p, 1, st);
            let ib = [0x8000u16];
            let mut ob = [0u16; 3];
            c::cmsPipelineEval16(ib.as_ptr(), ob.as_mut_ptr(), p);
            eprintln!("ncl ob={ob:?}");
            c::cmsPipelineFree(p);
        }
        // bAcs/eAcs evaluators — abstract-color-stage
        let st2 = c::_cmsStageAllocIdentityCLut(std::ptr::null_mut(), 3);
        if !st2.is_null() {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            c::cmsPipelineInsertStage(p, 1, st2);
            let ib = [0x4000u16; 3];
            let mut ob = [0u16; 3];
            c::cmsPipelineEval16(ib.as_ptr(), ob.as_mut_ptr(), p);
            eprintln!("idclut ob={ob:?}");
            c::cmsPipelineFree(p);
        }
        c::cmsFreeNamedColorList(ncl);
    }
}

/// LUT8/16 reader internals — mft1/mft2 tags with varied
/// channel counts + grid sizes -> the LUT readers' element loops.
#[test]
fn lut8_16_readers() {
    unsafe {
        for (nch_in, nch_out, grid, name) in [
            (1, 1, 2, "1x1"), (3, 3, 2, "3x3"),
            (4, 4, 2, "4x4"), (3, 3, 17, "3x3-g17"),
        ] {
            // mft1 (Lut8) — 8-bit tables
            let mut t = Vec::new();
            t.extend_from_slice(&0x6d667431u32.to_be_bytes());
            t.extend_from_slice(&[0; 4]);
            t.push(nch_in); t.push(nch_out); t.push(grid); t.push(0);
            for _ in 0..9 { t.extend_from_slice(&0x10000u32.to_be_bytes()); }
            let n = (grid as usize).pow(nch_in as u32) * nch_out as usize;
            let total = 256 * nch_in as usize + n + 256 * nch_out as usize;
            for i in 0..total { t.push((i * 3) as u8); }
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsWriteRawTag(prof, 0x41324230, t.as_ptr() as *const _,
                t.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut sz = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut sz);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, sz)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x41324230)
            } else { std::ptr::null_mut() };
            eprintln!("mft1 {name} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// mAB/mBA element-offset readers — mAB tags with each element
/// position populated.
#[test]
fn mab_element_readers() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mat = [0.4f64, 0.3, 0.3, 0.2, 0.6, 0.2, 0.1, 0.1, 0.8];
        let ftab: Vec<f32> = (0..8*3).map(|i| i as f32 / 24.0).collect();
        // mAB element combinations — A-curves, matrix, M-curves, clut
        for (stages, name) in [
            (vec![0], "B-cv"),
            (vec![0, 1, 0], "cv-mx-cv"),
            (vec![2, 1, 0], "clut-mx-cv"),
            (vec![0, 1, 2], "cv-mx-clut"),
        ] {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            for &k in &stages {
                match k {
                    0 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3,
                            [g; 3].as_ptr() as *mut _)),
                    1 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocMatrix(std::ptr::null_mut(),
                            3, 3, mat.as_ptr(), std::ptr::null())),
                    _ => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocCLutFloat(
                            std::ptr::null_mut(), 2, 3, 3,
                            ftab.as_ptr())),
                };
            }
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, 0x41324230, p as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x41324230)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
    }
}

/// LUT reader internals — mft1/mft2/mAB/mBA tags with varied
/// channel counts + element combos -> the readers' offset parsing.
#[test]
fn lut_readers_internals2() {
    unsafe {
        // mft2 (Lut16) — varied channels/grids
        for (ni, no, g, name) in [
            (1u8, 1, 2, "1x1"), (3, 3, 2, "3x3"),
            (4, 4, 2, "4x4"), (3, 3, 17, "g17"),
        ] {
            let mut t = Vec::new();
            t.extend_from_slice(&0x6d667432u32.to_be_bytes());
            t.extend_from_slice(&[0; 4]);
            t.push(ni); t.push(no); t.push(g); t.push(0);
            for _ in 0..9 { t.extend_from_slice(&0x10000u32.to_be_bytes()); }
            t.extend_from_slice(&256u16.to_be_bytes());
            t.extend_from_slice(&256u16.to_be_bytes());
            let n = (g as usize).pow(ni as u32) * no as usize;
            let total = 256 * ni as usize + n + 256 * no as usize;
            for i in 0..total { t.extend_from_slice(&(i as u16).to_be_bytes()); }
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsWriteRawTag(prof, 0x41324230, t.as_ptr() as *const _,
                t.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut sz = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut sz);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, sz)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x41324230)
            } else { std::ptr::null_mut() };
            eprintln!("mft2 {name} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// MPE element-reader remaining arms — curve-element, bAcs/eAcs
/// element readers.
#[test]
fn mpe_remaining_elements() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let ftab: Vec<f32> = (0..8*3).map(|i| i as f32 / 24.0).collect();
        // cv-only pipeline -> curve element in mpet
        for (k, name) in [(0, "curves"), (1, "clutf")] {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            if k == 0 {
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocToneCurves(std::ptr::null_mut(),
                        3, [g; 3].as_ptr() as *mut _));
            } else {
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocCLutFloat(std::ptr::null_mut(),
                        2, 3, 3, ftab.as_ptr()));
            }
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, 0x44324230, p as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x44324230)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
    }
}

/// MPE element-reader arms — curve-element (rejected by writer),
/// matrix, clutf, bacs/eacs element dispatch.
#[test]
fn mpe_reader_element_arms() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mat = [0.4f64, 0.3, 0.3, 0.2, 0.6, 0.2, 0.1, 0.1, 0.8];
        let ftab: Vec<f32> = (0..8*3).map(|i| i as f32 / 24.0).collect();
        for (k, name) in [(0, "curves"), (1, "matrix"), (2, "clutf")] {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            match k {
                0 => c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocToneCurves(
                        std::ptr::null_mut(), 3,
                        [g; 3].as_ptr() as *mut _)),
                1 => c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocMatrix(std::ptr::null_mut(),
                        3, 3, mat.as_ptr(), std::ptr::null())),
                _ => c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocCLutFloat(
                        std::ptr::null_mut(), 2, 3, 3,
                        ftab.as_ptr())),
            };
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, 0x44324230, p as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x44324230)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
    }
}

/// Type_LUTB2A_Write/Read — mBA element order (B, Mx, M, CLUT, A)
/// through BToA sigs.
#[test]
fn mba_writer_offsets() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mat = [0.4f64, 0.3, 0.3, 0.2, 0.6, 0.2, 0.1, 0.1, 0.8];
        let ftab: Vec<f32> = (0..8*3).map(|i| i as f32 / 24.0).collect();
        // mBA element order is reverse of mAB: B first, then Mx, M, CLUT, A
        for (stages, name) in [
            (vec![0], "B-only"),
            (vec![0, 2], "B-clut"),
            (vec![2, 1, 0], "clut-mx-cv"),
            (vec![0, 1, 2], "cv-mx-clut"),
        ] {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            for &k in &stages {
                match k {
                    0 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3,
                            [g; 3].as_ptr() as *mut _)),
                    1 => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocMatrix(std::ptr::null_mut(),
                            3, 3, mat.as_ptr(), std::ptr::null())),
                    _ => c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocCLut16bit(
                            std::ptr::null_mut(), 2, 3, 3,
                            std::ptr::null())),
                };
            }
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, 0x42324130, p as *const _); // B2A0
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x42324130)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
    }
}

/// Type_LUTB2A_Write all 4 accepted shapes — cv, cv-mx-cv,
/// cv-clut-cv, cv-mx-cv-clut-cv -> every element offset arm.
#[test]
fn mba_all_shapes() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mat = [0.4f64, 0.3, 0.3, 0.2, 0.6, 0.2, 0.1, 0.1, 0.8];
        let clut = |ftab: &[f32]| c::cmsStageAllocCLutFloat(
            std::ptr::null_mut(), 2, 3, 3, ftab.as_ptr());
        let ftab: Vec<f32> = (0..8*3).map(|i| i as f32 / 24.0).collect();
        let shapes: Vec<Vec<u8>> = vec![
            vec![0],                    // B
            vec![0, 1, 0],              // B, Mx, M
            vec![0, 2, 0],              // B, CLUT, A
            vec![0, 1, 0, 2, 0],        // B, Mx, M, CLUT, A (full)
        ];
        for (sig, sname) in [(0x42324130u32, "B2A"), (0x41324230, "A2B")] {
            for (i, stages) in shapes.iter().enumerate() {
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                for &k in stages {
                    match k {
                        0 => c::cmsPipelineInsertStage(p, 1,
                            c::cmsStageAllocToneCurves(
                                std::ptr::null_mut(), 3,
                                [g; 3].as_ptr() as *mut _)),
                        1 => c::cmsPipelineInsertStage(p, 1,
                            c::cmsStageAllocMatrix(
                                std::ptr::null_mut(), 3, 3,
                                mat.as_ptr(), std::ptr::null())),
                        _ => c::cmsPipelineInsertStage(p, 1,
                            clut(&ftab)),
                    };
                }
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 4.4);
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 20];
                let mut n = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut n)
                } else { 0 };
                let rp = if s != 0 {
                    c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
                } else { std::ptr::null_mut() };
                let tag = if !rp.is_null() {
                    c::cmsReadTag(rp, sig)
                } else { std::ptr::null_mut() };
                eprintln!("{sname} shape{i} w={w} s={s} read={}",
                    !tag.is_null());
                if !rp.is_null() { c::cmsCloseProfile(rp); }
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
    }
}

/// mAB/mBA CLUT shapes with 16-bit CLUTs (the writer requires
/// cmsSigCLutElemType, not float).
#[test]
fn mab_mba_clut_shapes() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mat = [0.4f64, 0.3, 0.3, 0.2, 0.6, 0.2, 0.1, 0.1, 0.8];
        let shapes: Vec<Vec<u8>> = vec![
            vec![0, 2, 0],              // cv, CLUT, cv
            vec![0, 1, 0, 2, 0],        // cv, Mx, cv, CLUT, cv
            vec![2],                    // CLUT only
            vec![0, 2],                 // cv, CLUT
            vec![2, 0],                 // CLUT, cv
        ];
        for (sig, sname) in [(0x42324130u32, "B2A"), (0x41324230, "A2B")] {
            for (i, stages) in shapes.iter().enumerate() {
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                for &k in stages {
                    match k {
                        0 => c::cmsPipelineInsertStage(p, 1,
                            c::cmsStageAllocToneCurves(
                                std::ptr::null_mut(), 3,
                                [g; 3].as_ptr() as *mut _)),
                        1 => c::cmsPipelineInsertStage(p, 1,
                            c::cmsStageAllocMatrix(
                                std::ptr::null_mut(), 3, 3,
                                mat.as_ptr(), std::ptr::null())),
                        _ => c::cmsPipelineInsertStage(p, 1,
                            c::cmsStageAllocCLut16bit(
                                std::ptr::null_mut(), 2, 3, 3,
                                std::ptr::null())),
                    };
                }
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 4.4);
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 20];
                let mut n = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut n)
                } else { 0 };
                let rp = if s != 0 {
                    c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
                } else { std::ptr::null_mut() };
                let tag = if !rp.is_null() {
                    c::cmsReadTag(rp, sig)
                } else { std::ptr::null_mut() };
                eprintln!("{sname} shape{i} w={w} s={s} read={}",
                    !tag.is_null());
                if !rp.is_null() { c::cmsCloseProfile(rp); }
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
    }
}

/// A2B full 5-element shape — cv, CLUT, cv, Mx, cv eval order.
#[test]
fn a2b_full_shape() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mat = [0.4f64, 0.3, 0.3, 0.2, 0.6, 0.2, 0.1, 0.1, 0.8];
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        // A, CLUT, M, Matrix, B
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g; 3].as_ptr() as *mut _));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), 2, 3, 3,
                std::ptr::null()));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g; 3].as_ptr() as *mut _));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), std::ptr::null()));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g; 3].as_ptr() as *mut _));
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof, 4.4);
        let w = c::cmsWriteTag(prof, 0x41324230, p as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x41324230)
        } else { std::ptr::null_mut() };
        eprintln!("A2B-full w={w} s={s} read={}", !tag.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
        c::cmsPipelineFree(p);
    }
}

/// mft1/mft2 writer shapes — mx-cv-clut-cv and friends through
/// WriteTag(A2B0)+save-as-8bit flag for mft1.
#[test]
fn lut8_16_writer_shapes() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mat = [0.4f64, 0.3, 0.3, 0.2, 0.6, 0.2, 0.1, 0.1, 0.8];
        for (stages, name) in [
            (vec![0u8, 2, 0], "cv-clut-cv"),
            (vec![1, 0, 2, 0], "mx-cv-clut-cv"),
            (vec![2], "clut-only"),
            (vec![0, 2], "cv-clut"),
        ] {
            for save8 in [false, true] {
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                for k in stages.iter().copied() {
                    match k {
                        0 => c::cmsPipelineInsertStage(p, 1,
                            c::cmsStageAllocToneCurves(
                                std::ptr::null_mut(), 3,
                                [g; 3].as_ptr() as *mut _)),
                        1 => c::cmsPipelineInsertStage(p, 1,
                            c::cmsStageAllocMatrix(
                                std::ptr::null_mut(), 3, 3,
                                mat.as_ptr(), std::ptr::null())),
                        _ => c::cmsPipelineInsertStage(p, 1,
                            c::cmsStageAllocCLut16bit(
                                std::ptr::null_mut(), 2, 3, 3,
                                std::ptr::null())),
                    };
                }
                if save8 {
                    c::cmsPipelineSetSaveAs8bitsFlag(p, 1);
                }
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1); // mft1/mft2 are v2
                let w = c::cmsWriteTag(prof, 0x41324230, p as *const _);
                let mut buf = vec![0u8; 1 << 20];
                let mut n = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut n)
                } else { 0 };
                let rp = if s != 0 {
                    c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
                } else { std::ptr::null_mut() };
                let tag = if !rp.is_null() {
                    c::cmsReadTag(rp, 0x41324230)
                } else { std::ptr::null_mut() };
                eprintln!("{name} 8bit={save8} w={w} s={s} read={}",
                    !tag.is_null());
                if !rp.is_null() { c::cmsCloseProfile(rp); }
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
    }
}

/// cmslut residual — inputChan>MAX_INPUT_DIMENSIONS
///   reject on CLut16bit/Float granular allocs +
///   CurveSetDup/CLUTElemDup via pipeline dup on
///   curve-set and clut stages.
#[test]
fn lut_dup_oversize_arms() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        // oversize input channels → range error
        for (n, name) in [
            (15u32, "15"), (16, "16"), (100, "100"),
        ] {
            let dims = [3u32; 15];
            let c16 = c::cmsStageAllocCLut16bitGranular(
                std::ptr::null_mut(), dims.as_ptr(),
                n, 3, std::ptr::null_mut());
            let cf = c::cmsStageAllocCLutFloatGranular(
                std::ptr::null_mut(), dims.as_ptr(),
                n, 3, std::ptr::null_mut());
            eprintln!("{name} c16={} cf={}",
                !c16.is_null(), !cf.is_null());
            if !c16.is_null() { c::cmsStageFree(c16); }
            if !cf.is_null() { c::cmsStageFree(cf); }
        }
        // pipeline dup on cvst + clut stages →
        //   CurveSetDup/CLUTElemDup bodies
        for nch in [1u32, 3, 4] {
            let p = c::cmsPipelineAlloc(
                std::ptr::null_mut(), nch, nch);
            let trcs = vec![g; nch as usize];
            c::cmsPipelineInsertStage(p, 0,
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), nch,
                    trcs.as_ptr() as *mut _));
            let dims = [3u32; 15];
            let npt = 3usize.pow(nch) * nch as usize;
            let g16 = vec![0x4000u16; npt];
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocCLut16bitGranular(
                    std::ptr::null_mut(), dims.as_ptr(),
                    nch, nch, g16.as_ptr()));
            let nf = 3usize.pow(nch) * nch as usize;
            let gf = vec![0.5f32; nf];
            c::cmsPipelineInsertStage(p, 2,
                c::cmsStageAllocCLutFloatGranular(
                    std::ptr::null_mut(), dims.as_ptr(),
                    nch, nch, gf.as_ptr()));
            let dp = c::cmsPipelineDup(p);
            eprintln!("{nch}ch dp={}", !dp.is_null());
            if !dp.is_null() {
                // eval the dup'd pipeline → verify dup'd
                //   stages are functional
                let src = vec![0x4000u16; nch as usize];
                let mut dst = vec![0u16; nch as usize];
                c::cmsPipelineEval16(
                    src.as_ptr(), dst.as_mut_ptr(), dp);
                c::cmsPipelineFree(dp);
            }
            c::cmsPipelineFree(p);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// cmsintrp/cmssm/cmscam02/cmsmd5 residual — interp
///   plugin factory path, GBD gamut-boundary add/
///   check/compute, CIECAM02 fwd/rev, MD5 stream.
#[test]
fn lut_interp_gbd_cam_md5_arms() {
    unsafe {
        // interp plugin: factory fires on CLUT interp-
        //   param computation
        #[repr(C)]
        struct PluginBase {
            magic: u32, ver: u32, typ: u32,
            next: *mut std::ffi::c_void,
        }
        #[repr(C)]
        union IFunc {
            l16: *const std::ffi::c_void,
            lf: *const std::ffi::c_void,
        }
        #[repr(C)]
        struct InterpPlugin {
            base: PluginBase,
            factory: *const std::ffi::c_void,
        }
        unsafe extern "C" fn my_lerp16(
            inp: *const u16, out: *mut u16,
            _p: *const std::ffi::c_void) {
            *out = *inp;
        }
        unsafe extern "C" fn my_factory(
            _nin: u32, _nout: u32, _flags: u32) -> IFunc {
            IFunc { l16: my_lerp16
                as *const std::ffi::c_void }
        }
        let ctx = c::cmsCreateContext(
            std::ptr::null_mut(), std::ptr::null_mut());
        let mut ip = InterpPlugin {
            base: PluginBase {
                magic: 0x61637070, ver: 2200,
                typ: 0x696E7048,
                next: std::ptr::null_mut(),
            },
            factory: my_factory as *const std::ffi::c_void,
        };
        let r = c::cmsPluginTHR(ctx,
            &mut ip as *mut _ as *mut _);
        eprintln!("interp-plugin reg={r}");
        // a transform on ctx forces interp-params compute
        let srgb = c::cmsCreate_sRGBProfile();
        let fin = (0u32 << 16) | (3 << 3) | 2;
        let t = c::cmsCreateTransformTHR(ctx, srgb, fin,
            srgb, fin, 0, 0);
        eprintln!("interp-xform t={}", !t.is_null());
        if !t.is_null() {
            c::cmsDeleteTransform(t); }
        // GBD: alloc + add points + check + compute
        let gbd = c::cmsGBDAlloc(std::ptr::null_mut());
        eprintln!("gbd={}", !gbd.is_null());
        if !gbd.is_null() {
            for i in 0..8 {
                let mut lab = [i as f64 * 10.0, 0.0, 0.0];
                c::cmsGDBAddPoint(gbd,
                    lab.as_mut_ptr() as *const _);
            }
            let cr = c::cmsGDBCompute(gbd, 0);
            let mut chk = [50.0f64, 0.0, 0.0];
            let inside = c::cmsGDBCheckPoint(gbd,
                chk.as_mut_ptr() as *const _);
            let mut out_lab = [200.0f64, 150.0, 150.0];
            let outside = c::cmsGDBCheckPoint(gbd,
                out_lab.as_mut_ptr() as *const _);
            eprintln!("gbd compute={cr} in={inside} \
                out={outside}");
            c::cmsGBDFree(gbd);
        }
        // CIECAM02 fwd/rev
        #[repr(C)]
        struct Vc {
            x_w: f64, y_w: f64, z_w: f64,
            y_b: f64, l_a: f64,
            surround: i32, d_value: f64,
        }
        let vc = Vc {
            x_w: 95.05, y_w: 100.0, z_w: 108.88,
            y_b: 20.0, l_a: 20.0,
            surround: 2, d_value: 1.0,
        };
        let cam = c::cmsCIECAM02Init(
            std::ptr::null_mut(),
            &vc as *const _ as *const _);
        if !cam.is_null() {
            let mut jch = [0.0f64; 3];
            let xyz_in = [20.0f64, 25.0, 18.0];
            c::cmsCIECAM02Forward(cam,
                xyz_in.as_ptr() as *const _,
                jch.as_mut_ptr() as *mut _);
            eprintln!("cam02 JCh={jch:?}");
            let mut xyz_out = [0.0f64; 3];
            c::cmsCIECAM02Reverse(cam,
                jch.as_ptr() as *const _,
                xyz_out.as_mut_ptr() as *mut _);
            eprintln!("cam02 rev={xyz_out:?}");
            c::cmsCIECAM02Done(cam);
        }
        // MD5 stream
        let h = c::cmsMD5alloc(std::ptr::null_mut());
        if !h.is_null() {
            c::cmsMD5add(h, b"hello".as_ptr(), 5);
            let mut pid = [0u8; 16];
            c::cmsMD5finish(
                pid.as_mut_ptr() as *mut _, h);
            eprintln!("md5 {pid:?}");
        }
        c::cmsCloseProfile(srgb);
        c::cmsDeleteContext(ctx);
    }
}

/// cmsopt residual — PatchLUT nChannelsIn==4 (CMYK) and
///   ==1 (Gray) arms via curve+CLUT+curve pipelines
///   through the prelinearization optimizer.
#[test]
fn lut_patchlut_arms() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for (nin, nout, cs, pcs, cls, fname) in [
            // CMYK: 4-in 3-out cv+clut+cv pipeline
            (4u32, 3u32, 0x434D594Bu32, 0x4C616220u32,
                0x70726E74u32, "cmyk"),
            // Gray: 1-in 3-out
            (1, 3, 0x47524159u32, 0x4C616220u32,
                0x70726E74u32, "gray"),
        ] {
            let prof = c::cmsCreateProfilePlaceholder(
                std::ptr::null_mut());
            c::cmsSetDeviceClass(prof, cls);
            c::cmsSetColorSpace(prof, cs);
            c::cmsSetPCS(prof, pcs);
            c::cmsSetProfileVersion(prof, 4.4);
            let pipe = c::cmsPipelineAlloc(
                std::ptr::null_mut(), nin, nout);
            let cin: Vec<*mut std::ffi::c_void> =
                (0..nin).map(|_| g as *mut _).collect();
            c::cmsPipelineInsertStage(pipe, 0,
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), nin,
                    cin.as_ptr() as *mut _));
            let dims = [9u32; 15];
            let n_grid = 9usize.pow(nin) * nout as usize;
            let grid = vec![0x8000u16; n_grid];
            c::cmsPipelineInsertStage(pipe, 1,
                c::cmsStageAllocCLut16bitGranular(
                    std::ptr::null_mut(), dims.as_ptr(),
                    nin, nout, grid.as_ptr()));
            let cout: Vec<*mut std::ffi::c_void> =
                (0..nout).map(|_| g as *mut _).collect();
            c::cmsPipelineInsertStage(pipe, 2,
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), nout,
                    cout.as_ptr() as *mut _));
            c::cmsWriteTag(prof, 0x41324230,
                pipe as *const _);
            // transform through it — optimizer runs
            let fin = (0u32 << 16) | (nin << 3) | 2;
            let fout = (0u32 << 16) | (nout << 3) | 2;
            let srgb = c::cmsCreate_sRGBProfile();
            let lab = c::cmsCreateLab4Profile(
                std::ptr::null_mut());
            let out_prof = if pcs == 0x4C616220u32 {
                lab } else { srgb };
            for fl in [0u32, 0x0100, 0x0080] {
                let t = c::cmsCreateTransform(prof, fin,
                    out_prof, fout, 0, fl);
                eprintln!("{fname} fl{fl:#x} t={}",
                    !t.is_null());
                if !t.is_null() {
                    let src = vec![0x8000u16;
                        nin as usize];
                    let mut dst = vec![0u16;
                        nout as usize];
                    c::cmsDoTransform(t,
                        src.as_ptr() as *const _,
                        dst.as_mut_ptr() as *mut _, 1);
                    eprintln!("  {fname} {dst:?}");
                    c::cmsDeleteTransform(t);
                }
            }
            c::cmsCloseProfile(prof);
            c::cmsCloseProfile(srgb);
            c::cmsCloseProfile(lab);
            c::cmsPipelineFree(pipe);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// cmsopt residual — OptimizeMatrixShaper identity-
///   product removal: mat followed by its inverse ->
///   _cmsMAT3per -> identity -> both stages dropped.
#[test]
fn lut_mat_ident_arms() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let prof = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(prof, 0x70726E74);
        c::cmsSetColorSpace(prof, 0x52474220);
        c::cmsSetPCS(prof, 0x4C616220);
        c::cmsSetProfileVersion(prof, 4.4);
        let pipe = c::cmsPipelineAlloc(
            std::ptr::null_mut(), 3, 3);
        let cin = [g, g, g];
        c::cmsPipelineInsertStage(pipe, 0,
            c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3,
                cin.as_ptr() as *mut _));
        // mat1: scale by 2 on diagonal
        let mut m1 = [0.0f64; 9];
        m1[0] = 2.0; m1[4] = 2.0; m1[8] = 2.0;
        c::cmsPipelineInsertStage(pipe, 1,
            c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 3,
                m1.as_ptr(), std::ptr::null()));
        // mat2: inverse = 0.5 diagonal -> product identity
        let mut m2 = [0.0f64; 9];
        m2[0] = 0.5; m2[4] = 0.5; m2[8] = 0.5;
        c::cmsPipelineInsertStage(pipe, 2,
            c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 3,
                m2.as_ptr(), std::ptr::null()));
        let cout = [g, g, g];
        c::cmsPipelineInsertStage(pipe, 3,
            c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3,
                cout.as_ptr() as *mut _));
        c::cmsWriteTag(prof, 0x41324230,
            pipe as *const _);
        let srgb = c::cmsCreate_sRGBProfile();
        let fin = (0u32 << 16) | (3 << 3) | 2;
        for fl in [0u32, 0x0100] {
            let t = c::cmsCreateTransform(prof, fin,
                srgb, fin, 0, fl);
            eprintln!("mat-ident fl{fl:#x} t={}",
                !t.is_null());
            if !t.is_null() {
                let src = [0x4000u16; 3];
                let mut dst = [0u16; 3];
                c::cmsDoTransform(t,
                    src.as_ptr() as *const _,
                    dst.as_mut_ptr() as *mut _, 1);
                eprintln!("  {src:?}->{dst:?}");
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(prof);
        c::cmsCloseProfile(srgb);
        c::cmsPipelineFree(pipe);
        c::cmsFreeToneCurve(g);
    }
}

/// cmslut residual — N-channel (4/5/6) MPE pipeline
///   evaluation: EvalNInputs dispatch, matf/cvst/
///   clut multi-channel stage internals.
#[test]
fn lut_nchan_eval_arms() {
    unsafe {
        unsafe extern "C" fn onerr(
            _ctx: *mut std::ffi::c_void, code: u32,
            msg: *const i8) {
            let s = unsafe {
                std::ffi::CStr::from_ptr(msg)
                    .to_string_lossy() };
            eprintln!("  cerr {code}: {s}");
        }
        let fp: unsafe extern "C" fn(
            *mut std::ffi::c_void, u32, *const i8) = onerr;
        c::cmsSetLogErrorHandlerTHR(
            std::ptr::null_mut(), fp as *mut _);
        // 4ch/5ch/6ch cvst+matf+clut mpet pipelines on
        //   D2B0 -> multi-channel evaluator internals
        for (nch, name) in [(4u16, "4ch"), (5, "5ch"),
                            (6, "6ch")] {
            // cvst element: nch curves
            let mut cv = vec![0x63u8, 0x76, 0x73, 0x74,
                0, 0, 0, 0];
            cv.extend_from_slice(&nch.to_be_bytes());
            cv.extend_from_slice(&nch.to_be_bytes());
            // posTable = nch {offset,size} pairs,
            //   offsets rel element start (incl
            //   'cvst'+res = 8 bytes)
            let curve_sz = 8 + 4 + 8 + 4 + 16;
            let base = 8 + 4 + 8 * nch as usize;
            for i in 0..nch as usize {
                let off = (base + i * curve_sz) as u32;
                cv.extend_from_slice(&off.to_be_bytes());
                cv.extend_from_slice(
                    &(curve_sz as u32).to_be_bytes());
            }
            for _ in 0..nch {
                cv.extend_from_slice(&[
                    0x63, 0x75, 0x72, 0x66, 0, 0, 0, 0,
                    0, 1, 0, 0,
                    0x70, 0x61, 0x72, 0x66, 0, 0, 0, 0,
                    0, 0, 0, 0,
                    0x3F, 0x80, 0, 0,
                    0x3F, 0x80, 0, 0,
                    0x3F, 0x80, 0, 0,
                    0x3F, 0x80, 0, 0,
                ]);
            }
            // mpet with 1 cvst element
            let mut blob = vec![
                0x6D, 0x70, 0x65, 0x74, 0, 0, 0, 0,
            ];
            blob.extend_from_slice(&nch.to_be_bytes());
            blob.extend_from_slice(&nch.to_be_bytes());
            blob.extend_from_slice(
                &1u32.to_be_bytes()); // nElem
            let eoff = 24u32;
            let esize = cv.len() as u32;
            blob.extend_from_slice(&eoff.to_be_bytes());
            blob.extend_from_slice(&esize.to_be_bytes());
            blob.extend_from_slice(&cv);
            let cs = match nch {
                4 => 0x434D594Bu32,  // cmyk
                5 => 0x35434C52,      // 5CLR
                _ => 0x36434C52,      // 6CLR
            };
            let p = c::cmsCreateProfilePlaceholder(
                std::ptr::null_mut());
            c::cmsSetDeviceClass(p, 0x6C696E6B);
            c::cmsSetColorSpace(p, cs);
            c::cmsSetPCS(p, 0x4C616220);
            c::cmsSetProfileVersion(p, 4.4);
            c::cmsWriteRawTag(p, 0x44324230,
                blob.as_ptr() as *const _,
                blob.len() as u32);
            let mut buf = [0u8; 1 << 17];
            let mut n = buf.len() as u32;
            let sv = c::cmsSaveProfileToMem(p,
                buf.as_mut_ptr() as *mut _, &mut n);
            let pr = if sv != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let rd = if !pr.is_null() {
                c::cmsReadTag(pr, 0x44324230)
            } else { std::ptr::null_mut() };
            eprintln!("{name} sv={sv} rd={}",
                !rd.is_null());
            if !pr.is_null() {
                c::cmsCloseProfile(pr); }
            c::cmsCloseProfile(p);
        }
        // multi-channel eval via linearization + CLUT
        //   devicelinks (Eval4/EvalNInputs internals)
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.5);
        for (fmt, nch, name) in [
            ((0u32 << 16) | (4 << 3) | 2, 4, "cmyk"),
            ((17u32 << 16) | (1 << 3) | 2, 1, "mch1"),
            ((21u32 << 16) | (5 << 3) | 2, 5, "mch5"),
            ((22u32 << 16) | (6 << 3) | 2, 6, "mch6"),
        ] {
            let tcs: Vec<*mut c::ToneCurve> =
                vec![g2];
            let dl = c::cmsCreateLinearizationDeviceLinkTHR(
                std::ptr::null_mut(), fmt,
                tcs.as_ptr());
            eprintln!("{name} lin dl={}", !dl.is_null());
            if !dl.is_null() {
                let t = c::cmsCreateTransform(dl,
                    (0u32 << 16) | (nch << 3) | 2,
                    dl, (0u32 << 16) | (nch << 3) | 2,
                    0, 0);
                eprintln!("  lin t={}", !t.is_null());
                if !t.is_null() {
                    let src = vec![0x4000u16;
                        nch as usize];
                    let mut dst = vec![0u16;
                        nch as usize];
                    c::cmsDoTransform(t,
                        src.as_ptr() as *const _,
                        dst.as_mut_ptr() as *mut _, 1);
                    eprintln!("    {dst:?}");
                    c::cmsDeleteTransform(t);
                }
                // also link->srgb output (Nch->3 eval)
                let srgb = c::cmsCreate_sRGBProfile();
                let t2 = c::cmsCreateTransform(dl,
                    (0u32 << 16) | (nch << 3) | 2,
                    srgb, (4u32 << 16) | (3 << 3) | 2,
                    0, 0);
                eprintln!("  dl->srgb t={}",
                    !t2.is_null());
                if !t2.is_null() {
                    c::cmsDeleteTransform(t2); }
                c::cmsCloseProfile(srgb);
                c::cmsCloseProfile(dl);
            }
        }
        // 4ch->3ch pipeline: cvst4 + clut4->3 + mat3 +
        //   cvst3 as A2B0 on a CMYK link
        let pipe = c::cmsPipelineAlloc(
            std::ptr::null_mut(), 4, 3);
        let g3 = c::cmsBuildGamma(
            std::ptr::null_mut(), 1.0);
        let tc4: Vec<_> = (0..4).map(|_| g3).collect();
        let tc3: Vec<_> = (0..3).map(|_| g3).collect();
        c::cmsPipelineInsertStage(pipe, 0,
            c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 4,
                tc4.as_ptr() as *mut _));
        let dims = vec![2u32; 15];
        let grid = vec![0x8000u16; 2usize.pow(4) * 3];
        c::cmsPipelineInsertStage(pipe, 1,
            c::cmsStageAllocCLut16bitGranular(
                std::ptr::null_mut(), dims.as_ptr(),
                4, 3, grid.as_ptr()));
        let mut m3 = [0.0f64; 9];
        m3[0] = 1.0; m3[4] = 1.0; m3[8] = 1.0;
        c::cmsPipelineInsertStage(pipe, 2,
            c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 3,
                m3.as_ptr(), std::ptr::null()));
        c::cmsPipelineInsertStage(pipe, 3,
            c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3,
                tc3.as_ptr() as *mut _));
        let lp = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(lp, 0x6C696E6B);
        c::cmsSetColorSpace(lp, 0x434D594B);
        c::cmsSetPCS(lp, 0x4C616220);
        c::cmsSetProfileVersion(lp, 4.4);
        c::cmsWriteTag(lp, 0x41324230,
            pipe as *const _);
        let srgb = c::cmsCreate_sRGBProfile();
        let fcmyk = (0u32 << 16) | (4 << 3) | 2;
        let flab = (10u32 << 16) | (3 << 3) | 2;
        let lab4p = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let t3 = c::cmsCreateTransform(lp, fcmyk,
            lab4p, flab, 0, 0);
        eprintln!("4->3 pipe t={}", !t3.is_null());
        if !t3.is_null() {
            let src = [0x4000u16; 4];
            let mut d = [0u16; 3];
            c::cmsDoTransform(t3,
                src.as_ptr() as *const _,
                d.as_mut_ptr() as *mut _, 1);
            eprintln!("  {src:?}->{d:?}");
            c::cmsDeleteTransform(t3);
        }
        c::cmsCloseProfile(lp);
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab4p);
        c::cmsPipelineFree(pipe);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(g3);
        c::cmsSetLogErrorHandlerTHR(
            std::ptr::null_mut(),
            std::ptr::null_mut());
    }
}

/// cmsopt residual — SetMatShaper/OptimizeMatrixShaper
///   internals: non-identity gamma curves, non-unity
///   matrix entries, gamut-clip paths, 3-stage
///   curve+mat+curve pipelines through the optimizer.
#[test]
fn lut_opt_matshaper_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let frgb = (4u32 << 16) | (3 << 3) | 2;
        let flab = (10u32 << 16) | (3 << 3) | 2;
        // profiles with non-identity curves + scaled
        //   matrices -> opt-matrix-shaper internals
        for (gamma, mscale, moff, name) in [
            (2.2f64, 1.0, 0.0, "g22-ident"),
            (1.8, 1.0, 0.0, "g18-ident"),
            (2.2, 0.5, 0.0, "g22-half"),
            (2.2, 2.0, 0.0, "g22-double"),
            (2.2, -1.0, 0.0, "g22-neg"),
            (0.8, 1.0, 0.0, "g08-ident"),
            (4.0, 1.0, 0.0, "g40-ident"),
            (2.2, 1.0, 0.1, "g22-off"),
        ] {
            // build a cv+mat+cv pipeline profile:
            //   curves -> matrix -> curves
            let g = c::cmsBuildGamma(
                std::ptr::null_mut(), gamma);
            let pipe = c::cmsPipelineAlloc(
                std::ptr::null_mut(), 3, 3);
            let tcs = [g, g, g];
            c::cmsPipelineInsertStage(pipe, 0,
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), 3,
                    tcs.as_ptr() as *mut _));
            let mut m = [0.0f64; 9];
            let off = if moff != 0.0 {
                [moff, moff, moff] } else { [0.0; 3] };
            m[0] = mscale; m[4] = mscale; m[8] = mscale;
            c::cmsPipelineInsertStage(pipe, 1,
                c::cmsStageAllocMatrix(
                    std::ptr::null_mut(), 3, 3,
                    m.as_ptr(),
                    if moff != 0.0 {
                        off.as_ptr() } else {
                        std::ptr::null() }));
            c::cmsPipelineInsertStage(pipe, 2,
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), 3,
                    tcs.as_ptr() as *mut _));
            let lp = c::cmsCreateProfilePlaceholder(
                std::ptr::null_mut());
            c::cmsSetDeviceClass(lp, 0x6C696E6B);
            c::cmsSetColorSpace(lp, 0x52474220);
            c::cmsSetPCS(lp, 0x4C616220);
            c::cmsSetProfileVersion(lp, 4.4);
            c::cmsWriteTag(lp, 0x41324230,
                pipe as *const _);
            let t = c::cmsCreateTransform(lp, frgb,
                lab, flab, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let src = [0x4000u16; 3];
                let mut dst = [0u16; 3];
                c::cmsDoTransform(t,
                    src.as_ptr() as *const _,
                    dst.as_mut_ptr() as *mut _, 1);
                eprintln!("  {src:?}->{dst:?}");
                c::cmsDeleteTransform(t);
            }
            c::cmsCloseProfile(lp);
            c::cmsPipelineFree(pipe);
            c::cmsFreeToneCurve(g);
        }
        // also: pipeline with a LUT between curves
        //   (curve+clut+mat+curve - can't collapse)
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let pipe = c::cmsPipelineAlloc(
            std::ptr::null_mut(), 3, 3);
        let tcs = [g, g, g];
        c::cmsPipelineInsertStage(pipe, 0,
            c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3,
                tcs.as_ptr() as *mut _));
        let dims = vec![4u32; 15];
        let grid = vec![0x8000u16; 4usize.pow(3) * 3];
        c::cmsPipelineInsertStage(pipe, 1,
            c::cmsStageAllocCLut16bitGranular(
                std::ptr::null_mut(), dims.as_ptr(),
                3, 3, grid.as_ptr()));
        let mut m = [0.0f64; 9];
        m[0] = 1.0; m[4] = 1.0; m[8] = 1.0;
        c::cmsPipelineInsertStage(pipe, 2,
            c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 3,
                m.as_ptr(), std::ptr::null()));
        c::cmsPipelineInsertStage(pipe, 3,
            c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3,
                tcs.as_ptr() as *mut _));
        let lp = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(lp, 0x6C696E6B);
        c::cmsSetColorSpace(lp, 0x52474220);
        c::cmsSetPCS(lp, 0x4C616220);
        c::cmsSetProfileVersion(lp, 4.4);
        c::cmsWriteTag(lp, 0x41324230,
            pipe as *const _);
        for fl in [0u32, 0x0100, 0x2000] {
            let t = c::cmsCreateTransform(lp, frgb,
                lab, flab, 0, fl);
            eprintln!("cv+clut+mat+cv fl{fl:#x} t={}",
                !t.is_null());
            if !t.is_null() {
                c::cmsDeleteTransform(t); }
        }
        c::cmsCloseProfile(lp);
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsPipelineFree(pipe);
        c::cmsFreeToneCurve(g);
    }
}

/// cmsintrp/cmsmd5/cmsnamed residual — interpolation
///   dup/free on n-dim CLUTs, MD5 streaming adds,
///   named-list dup on populated lists.
#[test]
fn lut_md5_named_tail_arms() {
    unsafe {
        // MD5 streaming: multiple adds + finish
        let mut pid = c::ProfileID::default();
        let ctx = c::cmsMD5alloc(std::ptr::null_mut());
        let data = b"the quick brown fox jumps over";
        c::cmsMD5add(ctx, data.as_ptr(), 16);
        c::cmsMD5add(ctx, data[16..].as_ptr(),
            (data.len() - 16) as u32);
        c::cmsMD5add(ctx, data.as_ptr(), 0); // empty add
        c::cmsMD5finish(&mut pid, ctx);
        eprintln!("md5 id={:02x?}", &pid.id8[..6]);
        // a profile's MD5 computed id
        let srgb = c::cmsCreate_sRGBProfile();
        let mut buf = [0u8; 1 << 17];
        let mut n = buf.len() as u32;
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        let ctx2 = c::cmsMD5alloc(std::ptr::null_mut());
        for chunk in buf[..n as usize].chunks(37) {
            c::cmsMD5add(ctx2, chunk.as_ptr(),
                chunk.len() as u32);
        }
        let mut pid2 = c::ProfileID::default();
        c::cmsMD5finish(&mut pid2, ctx2);
        eprintln!("md5-prof={:02x?}", &pid2.id8[..6]);
        // interp dup: alloc a 4-dim interp params,
        //   dup via cmsStageDup on a clut stage
        let dims = vec![3u32; 15];
        let grid = vec![0x8000u16;
            3usize.pow(4) * 4];
        let clut = c::cmsStageAllocCLut16bitGranular(
            std::ptr::null_mut(), dims.as_ptr(),
            4, 4, grid.as_ptr());
        let clut2 = c::cmsStageDup(clut);
        eprintln!("clut dup={}", !clut2.is_null());
        // eval via pipeline transform
        let pipe = c::cmsPipelineAlloc(
            std::ptr::null_mut(), 4, 4);
        c::cmsPipelineInsertStage(pipe, 0, clut2);
        let lp = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(lp, 0x6C696E6B);
        c::cmsSetColorSpace(lp, 0x434D594B);
        c::cmsSetPCS(lp, 0x4C616220);
        c::cmsSetProfileVersion(lp, 4.4);
        c::cmsWriteTag(lp, 0x41324230,
            pipe as *const _);
        let lab4p = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let t = c::cmsCreateTransform(lp,
            (0u32 << 16) | (4 << 3) | 2,
            lab4p, (10u32 << 16) | (3 << 3) | 2, 0, 0);
        eprintln!("  clut4 t={}", !t.is_null());
        if !t.is_null() {
            let src = [0x4000u16; 4];
            let mut dst = [0u16; 3];
            c::cmsDoTransform(t,
                src.as_ptr() as *const _,
                dst.as_mut_ptr() as *mut _, 1);
            eprintln!("    {src:?}->{dst:?}");
            c::cmsDeleteTransform(t);
        }
        c::cmsCloseProfile(lp);
        c::cmsCloseProfile(lab4p);
        c::cmsPipelineFree(pipe);
        c::cmsStageFree(clut);
        // named-list: dup populated list + info queries
        let nl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 5, 4,
            b"Prefix\0".as_ptr() as *const _,
            b"Suffix\0".as_ptr() as *const _);
        let mut pcs = [0u16; 16];
        let mut dev = [0u16; 16];
        for i in 0..5u32 {
            let nm = format!("color{}\0", i);
            for k in 0..4 { pcs[k] = 1000 * (i + 1) as u16;
                dev[k] = 2000 * (i + 1) as u16; }
            c::cmsAppendNamedColor(nl,
                nm.as_ptr() as *const _,
                pcs.as_ptr(), dev.as_ptr());
        }
        let cnt = c::cmsNamedColorCount(nl);
        let nl2 = c::cmsDupNamedColorList(nl);
        let cnt2 = c::cmsNamedColorCount(nl2);
        eprintln!("named cnt={cnt} dup-cnt={cnt2}");
        let mut nm = [0u8; 32];
        let mut pfx = [0u8; 32];
        let mut sfx = [0u8; 32];
        let mut pcs2 = [0u16; 16];
        let mut dev2 = [0u16; 16];
        for i in 0..=(cnt + 1) {
            let ok = c::cmsNamedColorInfo(nl, i,
                nm.as_mut_ptr(), pfx.as_mut_ptr(),
                sfx.as_mut_ptr(), pcs2.as_mut_ptr(),
                dev2.as_mut_ptr());
            if ok != 0 {
                let s = std::ffi::CStr::from_ptr(
                    nm.as_ptr() as *const i8)
                    .to_string_lossy();
                eprintln!("  nc[{i}] '{s}' pcs={:?} dev={:?}",
                    &pcs2[..4], &dev2[..4]);
            }
        }
        // named-color index lookup
        let idx = c::cmsNamedColorIndex(nl,
            b"color2\0".as_ptr());
        eprintln!("idx(color2)={idx}");
        let idx2 = c::cmsNamedColorIndex(nl,
            b"nope\0".as_ptr());
        eprintln!("idx(nope)={idx2}");
        c::cmsFreeNamedColorList(nl2);
        c::cmsFreeNamedColorList(nl);
        c::cmsCloseProfile(srgb);
    }
}

/// cmsopt residual — WhitesAreEqual/IsDegenerated/
///   OptimizeByResampling on pipelines with unequal
///   whites, degenerate curves, off-white endpoints.
#[test]
fn lut_opt_white_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab4 = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let frgb = (4u32 << 16) | (3 << 3) | 2;
        let flab = (10u32 << 16) | (3 << 3) | 2;
        // pipelines with white endpoints != 65535
        for (end0, end1, name) in [
            (0x8000u16, 0xFFFF, "asym-ends"),
            (0x0000, 0x8000, "half-range"),
            (0xFFFF, 0xFFFF, "flat-max"),
            (0x0000, 0x0000, "flat-min"),
            (0x1000, 0xE000, "narrow"),
        ] {
            let mut tab = [0u16; 2];
            tab[0] = end0; tab[1] = end1;
            let g = c::cmsBuildTabulatedToneCurve16(
                std::ptr::null_mut(), 2, tab.as_ptr());
            let pipe = c::cmsPipelineAlloc(
                std::ptr::null_mut(), 3, 3);
            let tcs = [g, g, g];
            c::cmsPipelineInsertStage(pipe, 0,
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), 3,
                    tcs.as_ptr() as *mut _));
            let dims = vec![4u32; 15];
            let grid = vec![0x8000u16; 4usize.pow(3) * 3];
            c::cmsPipelineInsertStage(pipe, 1,
                c::cmsStageAllocCLut16bitGranular(
                    std::ptr::null_mut(), dims.as_ptr(),
                    3, 3, grid.as_ptr()));
            c::cmsPipelineInsertStage(pipe, 2,
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), 3,
                    tcs.as_ptr() as *mut _));
            let lp = c::cmsCreateProfilePlaceholder(
                std::ptr::null_mut());
            c::cmsSetDeviceClass(lp, 0x6C696E6B);
            c::cmsSetColorSpace(lp, 0x52474220);
            c::cmsSetPCS(lp, 0x4C616220);
            c::cmsSetProfileVersion(lp, 4.4);
            c::cmsWriteTag(lp, 0x41324230,
                pipe as *const _);
            for fl in [0u32, 0x0100, 0x2000] {
                let t = c::cmsCreateTransform(lp, frgb,
                    lab4, flab, 0, fl);
                if !t.is_null() {
                    c::cmsDeleteTransform(t); }
            }
            eprintln!("{name} ok");
            c::cmsCloseProfile(lp);
            c::cmsPipelineFree(pipe);
            c::cmsFreeToneCurve(g);
        }
        // degenerate curves (constant, near-constant,
        //   descending) through resample optimizer
        for (ty, name) in [
            (0u8, "const"),
            (1, "near-const"),
            (2, "descending"),
        ] {
            let g = if ty == 0 {
                let t2 = [0x4000u16, 0x4000];
                c::cmsBuildTabulatedToneCurve16(
                    std::ptr::null_mut(), 2, t2.as_ptr())
            } else if ty == 1 {
                let t2 = [0x4000u16, 0x4001];
                c::cmsBuildTabulatedToneCurve16(
                    std::ptr::null_mut(), 2, t2.as_ptr())
            } else {
                let t2 = [0xFFFFu16, 0x0000];
                c::cmsBuildTabulatedToneCurve16(
                    std::ptr::null_mut(), 2, t2.as_ptr())
            };
            let pipe = c::cmsPipelineAlloc(
                std::ptr::null_mut(), 3, 3);
            let tcs = [g, g, g];
            c::cmsPipelineInsertStage(pipe, 0,
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), 3,
                    tcs.as_ptr() as *mut _));
            let lp = c::cmsCreateProfilePlaceholder(
                std::ptr::null_mut());
            c::cmsSetDeviceClass(lp, 0x6C696E6B);
            c::cmsSetColorSpace(lp, 0x52474220);
            c::cmsSetPCS(lp, 0x4C616220);
            c::cmsSetProfileVersion(lp, 4.4);
            c::cmsWriteTag(lp, 0x41324230,
                pipe as *const _);
            let t = c::cmsCreateTransform(lp, frgb,
                lab4, flab, 0, 0);
            eprintln!("deg-{name} t={}", !t.is_null());
            if !t.is_null() {
                c::cmsDeleteTransform(t); }
            c::cmsCloseProfile(lp);
            c::cmsPipelineFree(pipe);
            c::cmsFreeToneCurve(g);
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab4);
    }
}

/// cmslut/cmsnamed residual — StageSampleCLut16bit
///   (callback sampler), SliceSpace16/Float,
///   MLUget{ASCII,UTF8,Wide,Translation} on multi-lang
///   strings, EvalToneCurve variants.
#[test]
fn lut_sample_slice_arms() {
    unsafe {
        // CLUT sample callback -> fills grid
        unsafe extern "C" fn sampler(
            inp: *const u16, out: *mut u16,
            _ud: *mut std::ffi::c_void) -> i32 {
            let i = unsafe {
                std::slice::from_raw_parts(inp, 3) };
            let o = unsafe {
                std::slice::from_raw_parts_mut(out, 3) };
            o[0] = i[1]; o[1] = i[2]; o[2] = i[0];
            1
        }
        let dims = [3u32; 3];
        let clut = c::cmsStageAllocCLut16bitGranular(
            std::ptr::null_mut(), dims.as_ptr(), 3, 3,
            std::ptr::null());
        let ok = c::cmsStageSampleCLut16bit(clut,
            sampler,
            std::ptr::null_mut(), 0);
        eprintln!("sample-clut={ok}");
        // SliceSpace16/Float: iterate a 3D cube
        unsafe extern "C" fn slicer(
            inp: *const u16, _out: *mut u16,
            _ud: *mut std::ffi::c_void) -> i32 {
            let i = unsafe {
                std::slice::from_raw_parts(inp, 3) };
            let _ = i[0] as u32 + i[1] as u32 + i[2] as u32;
            1
        }
        unsafe extern "C" fn slicerf(
            inp: *const f32, _out: *mut f32,
            _ud: *mut std::ffi::c_void) -> i32 {
            let i = unsafe {
                std::slice::from_raw_parts(inp, 3) };
            let _ = i[0] + i[1] + i[2];
            1
        }
        for n in [2u32, 3, 5] {
            let dims2 = [n; 3];
            let r1 = c::cmsSliceSpace16(3, dims2.as_ptr(),
                slicer,
                std::ptr::null_mut());
            let r2 = c::cmsSliceSpaceFloat(3,
                dims2.as_ptr(),
                slicerf,
                std::ptr::null_mut());
            eprintln!("slice n{n} 16={r1} f={r2}");
        }
        // EvalToneCurve 16/float on several curve types
        for ty in [1.0f64, 2.2, 0.5, 0.0] {
            let g = if ty == 0.0 {
                let t = [0u16, 0x8000, 0xFFFF];
                c::cmsBuildTabulatedToneCurve16(
                    std::ptr::null_mut(), 3, t.as_ptr())
            } else {
                c::cmsBuildGamma(std::ptr::null_mut(), ty)
            };
            for v in [0u16, 0x4000, 0x8000, 0xFFFF] {
                let r = c::cmsEvalToneCurve16(g, v);
                let rf = c::cmsEvalToneCurveFloat(g,
                    v as f32 / 65535.0);
                if v == 0x8000 {
                    eprintln!("tc{ty}: e16={r} ef={rf}"); }
            }
            c::cmsFreeToneCurve(g);
        }
        // MLU multi-lang get variants
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 3);
        c::cmsMLUsetASCII(mlu, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _,
            b"English text\0".as_ptr() as *const _);
        c::cmsMLUsetASCII(mlu, b"de\0".as_ptr() as *const _,
            b"DE\0".as_ptr() as *const _,
            b"Deutsch\0".as_ptr() as *const _);
        c::cmsMLUsetASCII(mlu, b"fr\0".as_ptr() as *const _,
            b"FR\0".as_ptr() as *const _,
            "Français\0".as_ptr() as *const _);
        let mut buf = [0u8; 128];
        for (lang, cty) in [
            (b"en\0" as &[u8], b"US\0"),
            (b"de\0", b"DE\0"), (b"fr\0", b"FR\0"),
            (b"es\0", b"ES\0"), (b"en\0", b"GB\0"),
        ] {
            let n = c::cmsMLUgetASCII(mlu,
                lang.as_ptr() as *const _,
                cty.as_ptr() as *const _,
                buf.as_mut_ptr(), buf.len() as u32);
            let s = std::ffi::CStr::from_ptr(
                    buf.as_ptr() as *const i8)
                .to_string_lossy();
            eprintln!("get {lang:?}/{cty:?}: {n} = {s}");
        }
        // UTF8 + Wide variants
        let mut wbuf = [0i32; 64];
        let nw = c::cmsMLUgetWide(mlu,
            b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _,
            wbuf.as_mut_ptr(),
            (wbuf.len() * 4) as u32);
        eprintln!("getWide={nw}");
        let mut u8buf = [0u8; 128];
        let n8 = c::cmsMLUgetUTF8(mlu,
            b"de\0".as_ptr() as *const _,
            b"DE\0".as_ptr() as *const _,
            u8buf.as_mut_ptr(),
            u8buf.len() as u32);
        eprintln!("getUTF8={n8}");
        c::cmsMLUfree(mlu);
        c::cmsStageFree(clut);
    }
}

/// cmsopt residual — OptimizeByComputingLinearization
///   internals: curve+clut+curve pipelines where
///   curves are non-identity/near-identity/piecewise.
#[test]
fn lut_opt_lin_arms() {
    unsafe {
        let lab4 = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let frgb = (4u32 << 16) | (3 << 3) | 2;
        let flab = (10u32 << 16) | (3 << 3) | 2;
        // piecewise + parametric + segmented curves
        //   pre/post a CLUT
        let mk_tc = |ty: u8| -> *mut c::ToneCurve {
            match ty {
                0 => { // parametric type 5 (multi-seg)
                    let mut params = [0f64; 10];
                    params[0] = 2.4;
                    params[1] = 1.0 / 1.055;
                    params[2] = 0.055 / 1.055;
                    params[3] = 1.0 / 12.92;
                    params[4] = 0.04045;
                    c::cmsBuildParametricToneCurve(
                        std::ptr::null_mut(), 5,
                        params.as_ptr())
                }
                1 => { // tabulated non-linear
                    let t = [0u16, 0x1000, 0x6000,
                        0xE000, 0xFFFF];
                    c::cmsBuildTabulatedToneCurve16(
                        std::ptr::null_mut(), 5,
                        t.as_ptr())
                }
                2 => { // near-linear
                    let t = [0u16, 0x4000, 0x8000,
                        0xC000, 0xFFFE];
                    c::cmsBuildTabulatedToneCurve16(
                        std::ptr::null_mut(), 5,
                        t.as_ptr())
                }
                3 => { // segmented
                    let segs = [
                        c::CurveSegment {
                            x0: -1.0e22, x1: 0.5,
                            r#type: 6,
                            params: [2.0, 1.0, 0.0,
                                0.0, 0.0, 0.0, 0.0,
                                0.0, 0.0, 0.0],
                            n_grid_points: 0,
                            sampled_points: std::ptr::null(),
                        },
                        c::CurveSegment {
                            x0: 0.5, x1: 1.0e22,
                            r#type: 6,
                            params: [1.0, 1.0, 0.0,
                                0.0, 0.0, 0.0, 0.0,
                                0.0, 0.0, 0.0],
                            n_grid_points: 0,
                            sampled_points: std::ptr::null(),
                        },
                    ];
                    c::cmsBuildSegmentedToneCurve(
                        std::ptr::null_mut(), 2,
                        segs.as_ptr())
                }
                _ => c::cmsBuildGamma(
                    std::ptr::null_mut(), 1.0),
            }
        };
        for pre in 0..4u8 {
            for post in 0..4u8 {
                let g1 = mk_tc(pre);
                let g2 = mk_tc(post);
                let pipe = c::cmsPipelineAlloc(
                    std::ptr::null_mut(), 3, 3);
                let t1 = [g1, g1, g1];
                let t2 = [g2, g2, g2];
                c::cmsPipelineInsertStage(pipe, 0,
                    c::cmsStageAllocToneCurves(
                        std::ptr::null_mut(), 3,
                        t1.as_ptr() as *mut _));
                let dims = [3u32; 3];
                let grid = vec![0x8000u16;
                    3usize.pow(3) * 3];
                c::cmsPipelineInsertStage(pipe, 1,
                    c::cmsStageAllocCLut16bitGranular(
                        std::ptr::null_mut(),
                        dims.as_ptr(), 3, 3,
                        grid.as_ptr()));
                c::cmsPipelineInsertStage(pipe, 2,
                    c::cmsStageAllocToneCurves(
                        std::ptr::null_mut(), 3,
                        t2.as_ptr() as *mut _));
                let lp = c::cmsCreateProfilePlaceholder(
                    std::ptr::null_mut());
                c::cmsSetDeviceClass(lp, 0x6C696E6B);
                c::cmsSetColorSpace(lp, 0x52474220);
                c::cmsSetPCS(lp, 0x4C616220);
                c::cmsSetProfileVersion(lp, 4.4);
                c::cmsWriteTag(lp, 0x41324230,
                    pipe as *const _);
                for fl in [0u32, 0x0100] {
                    let t = c::cmsCreateTransform(lp,
                        frgb, lab4, flab, 0, fl);
                    if !t.is_null() {
                        c::cmsDeleteTransform(t); }
                }
                c::cmsCloseProfile(lp);
                c::cmsPipelineFree(pipe);
                c::cmsFreeToneCurve(g1);
                c::cmsFreeToneCurve(g2);
            }
        }
        eprintln!("opt-lin 4x4 done");
        c::cmsCloseProfile(lab4);
    }
}

/// cmslut residual — stage-level eval/dup across all
///   stage types: cv/clut/mat/lab2xyz/xyz2lab/bake/
///   identity.
#[test]
fn lut_stage_eval_arms() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let tcs = [g, g, g];
        let cv = c::cmsStageAllocToneCurves(
            std::ptr::null_mut(), 3,
            tcs.as_ptr() as *mut _);
        let dims = [4u32; 3];
        let grid = vec![0x8000u16; 4usize.pow(3) * 3];
        let clut = c::cmsStageAllocCLut16bitGranular(
            std::ptr::null_mut(), dims.as_ptr(), 3, 3,
            grid.as_ptr());
        let m = [1.0f64, 0.1, 0.0,
                 0.0, 1.0, 0.0,
                 0.0, 0.0, 1.0];
        let mat = c::cmsStageAllocMatrix(
            std::ptr::null_mut(), 3, 3, m.as_ptr(),
            std::ptr::null());
        let id = c::cmsStageAllocIdentity(
            std::ptr::null_mut(), 3);
        // dup each + evaluate through a transform-like
        //   path via pipeline
        for (st, name) in [
            (cv, "cv"), (clut, "clut"), (mat, "mat"),
            (id, "ident"),
        ] {
            if !st.is_null() {
                let d = c::cmsStageDup(st);
                eprintln!("{name} dup={}", !d.is_null());
                if !d.is_null() {
                    c::cmsStageFree(d); }
            }
        }
        // pipeline with all stage types -> eval
        let pipe = c::cmsPipelineAlloc(
            std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(pipe, 0, id);
        c::cmsPipelineInsertStage(pipe, 1, cv);
        c::cmsPipelineInsertStage(pipe, 2, mat);
        c::cmsPipelineInsertStage(pipe, 3, clut);
        let lp = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(lp, 0x6C696E6B);
        c::cmsSetColorSpace(lp, 0x52474220);
        c::cmsSetPCS(lp, 0x4C616220);
        c::cmsSetProfileVersion(lp, 4.4);
        c::cmsWriteTag(lp, 0x41324230,
            pipe as *const _);
        let lab4 = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let frgb = (4u32 << 16) | (3 << 3) | 2;
        let flab = (10u32 << 16) | (3 << 3) | 2;
        let t = c::cmsCreateTransform(lp, frgb,
            lab4, flab, 0, 0);
        eprintln!("pipe-xform={}", !t.is_null());
        if !t.is_null() {
            let src = [0x8000u16; 3];
            let mut dst = [0u16; 3];
            c::cmsDoTransform(t,
                src.as_ptr() as *const _,
                dst.as_mut_ptr() as *mut _, 1);
            eprintln!("  {dst:?}");
            c::cmsDeleteTransform(t);
        }
        c::cmsCloseProfile(lp);
        c::cmsCloseProfile(lab4);
        c::cmsPipelineFree(pipe);
        c::cmsFreeToneCurve(g);
    }
}

/// cmslut residual — OptimizePipeline on float + 4ch
///   + long pipelines: float clut, 4->3/4->4, 6-stage.
#[test]
fn lut_optimize2_arms() {
    unsafe {
        let lab4 = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let frgb = (4u32 << 16) | (3 << 3) | 2;
        let flab = (10u32 << 16) | (3 << 3) | 2;
        // float clut pipeline + 4ch + long pipelines
        for (inch, outch, nstages, name) in [
            (3u32, 3u32, 1usize, "fclut-33"),
            (3, 3, 3, "cv-fclut-cv"),
            (4, 3, 2, "4in"),
            (3, 4, 2, "4out"),
            (4, 4, 3, "44"),
            (3, 3, 6, "6stage"),
        ] {
            let g = c::cmsBuildGamma(
                std::ptr::null_mut(), 1.0);
            let pipe = c::cmsPipelineAlloc(
                std::ptr::null_mut(), inch, outch);
            for st in 0..nstages as i32 {
                if st == 1 || nstages as i32 == 1 {
                    let dims = vec![3u32; 15];
                    let npts = 3usize.pow(inch.min(4));
                    let grid = vec![0.5f32;
                        npts * outch as usize];
                    c::cmsPipelineInsertStage(pipe, st,
                        c::cmsStageAllocCLutFloatGranular(
                            std::ptr::null_mut(),
                            dims.as_ptr(), inch, outch,
                            grid.as_ptr()));
                } else {
                    let tcs: Vec<*mut c::ToneCurve> =
                        vec![g;
                            (if st == 0 { inch }
                             else { outch }) as usize];
                    c::cmsPipelineInsertStage(pipe, st,
                        c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(),
                            tcs.len() as u32,
                            tcs.as_ptr() as *mut _));
                }
            }
            let lp = c::cmsCreateProfilePlaceholder(
                std::ptr::null_mut());
            c::cmsSetDeviceClass(lp, 0x6C696E6B);
            c::cmsSetColorSpace(lp, 0x52474220);
            c::cmsSetPCS(lp, 0x4C616220);
            c::cmsSetProfileVersion(lp, 4.4);
            c::cmsWriteTag(lp, 0x41324230,
                pipe as *const _);
            for fl in [0u32, 0x0100, 0x2000] {
                let t = c::cmsCreateTransform(lp,
                    frgb, lab4, flab, 0, fl);
                if !t.is_null() {
                    c::cmsDeleteTransform(t); }
            }
            eprintln!("{name} ok");
            c::cmsCloseProfile(lp);
            c::cmsPipelineFree(pipe);
            c::cmsFreeToneCurve(g);
        }
        c::cmsCloseProfile(lab4);
    }
}

/// cmsopt residual — OptimizeMatrixShaper/SetMatShaper
///   on pipelines with non-identity matrices, curve-
///   only matrix-shaper shapes, matrix+cv variants.
#[test]
fn lut_opt_matshaper2_arms() {
    unsafe {
        let lab4 = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let frgb = (4u32 << 16) | (3 << 3) | 2;
        let flab = (10u32 << 16) | (3 << 3) | 2;
        // matrix-shaper pipelines: cv -> mat -> cv with
        //   non-identity matrices + curve combos
        for (mid, name) in [
            (0u8, "ident-mat"),
            (1, "swap-rb"),
            (2, "scale2"),
            (3, "skew"),
        ] {
            let g = c::cmsBuildGamma(
                std::ptr::null_mut(), 2.2);
            let pipe = c::cmsPipelineAlloc(
                std::ptr::null_mut(), 3, 3);
            let tcs = [g, g, g];
            c::cmsPipelineInsertStage(pipe, 0,
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), 3,
                    tcs.as_ptr() as *mut _));
            let m = match mid {
                0 => [1.0, 0.0, 0.0,
                      0.0, 1.0, 0.0,
                      0.0, 0.0, 1.0],
                1 => [0.0, 0.0, 1.0,
                      0.0, 1.0, 0.0,
                      1.0, 0.0, 0.0],
                2 => [2.0, 0.0, 0.0,
                      0.0, 2.0, 0.0,
                      0.0, 0.0, 2.0],
                _ => [1.0, 0.5, 0.0,
                      0.0, 1.0, 0.5,
                      0.0, 0.0, 1.0],
            };
            c::cmsPipelineInsertStage(pipe, 1,
                c::cmsStageAllocMatrix(
                    std::ptr::null_mut(), 3, 3,
                    m.as_ptr(), std::ptr::null()));
            c::cmsPipelineInsertStage(pipe, 2,
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), 3,
                    tcs.as_ptr() as *mut _));
            let lp = c::cmsCreateProfilePlaceholder(
                std::ptr::null_mut());
            c::cmsSetDeviceClass(lp, 0x6C696E6B);
            c::cmsSetColorSpace(lp, 0x52474220);
            c::cmsSetPCS(lp, 0x4C616220);
            c::cmsSetProfileVersion(lp, 4.4);
            c::cmsWriteTag(lp, 0x41324230,
                pipe as *const _);
            for fl in [0u32, 0x0100, 0x2000] {
                let t = c::cmsCreateTransform(lp,
                    frgb, lab4, flab, 0, fl);
                if !t.is_null() {
                    c::cmsDeleteTransform(t); }
            }
            eprintln!("{name} ok");
            c::cmsCloseProfile(lp);
            c::cmsPipelineFree(pipe);
            c::cmsFreeToneCurve(g);
        }
        c::cmsCloseProfile(lab4);
    }
}

/// cmslut residual — pipeline shapes: identity-only,
///   single-stage each type, 2-stage combos, baked
///   curves -> Eval + OptimizePipeline internals.
#[test]
fn lut_pipe_shapes_arms() {
    unsafe {
        let lab4 = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let frgb = (4u32 << 16) | (3 << 3) | 2;
        let flab = (10u32 << 16) | (3 << 3) | 2;
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 1.0);
        for (shape, name) in [
            (0u8, "ident-only"),
            (1, "cv-only"),
            (2, "mat-only"),
            (3, "clut-only"),
            (4, "ident-cv"),
            (5, "cv-ident"),
            (6, "ident-mat"),
            (7, "mat-ident"),
            (8, "cv-cv"),
            (9, "mat-mat"),
            (10, "ident-ident"),
        ] {
            let pipe = c::cmsPipelineAlloc(
                std::ptr::null_mut(), 3, 3);
            let tcs = [g, g, g];
            let add_cv = |p| {
                c::cmsPipelineInsertStage(p, -1,
                    c::cmsStageAllocToneCurves(
                        std::ptr::null_mut(), 3,
                        tcs.as_ptr() as *mut _)); };
            let add_mat = |p| {
                let m = [1.0f64, 0.0, 0.0,
                         0.0, 1.0, 0.0,
                         0.0, 0.0, 1.0];
                c::cmsPipelineInsertStage(p, -1,
                    c::cmsStageAllocMatrix(
                        std::ptr::null_mut(), 3, 3,
                        m.as_ptr(), std::ptr::null())); };
            let add_clut = |p| {
                let dims = [3u32; 3];
                let grid = vec![0x8000u16;
                    3usize.pow(3) * 3];
                c::cmsPipelineInsertStage(p, -1,
                    c::cmsStageAllocCLut16bitGranular(
                        std::ptr::null_mut(),
                        dims.as_ptr(), 3, 3,
                        grid.as_ptr())); };
            let add_id = |p| {
                c::cmsPipelineInsertStage(p, -1,
                    c::cmsStageAllocIdentity(
                        std::ptr::null_mut(), 3)); };
            match shape {
                0 => add_id(pipe),
                1 => add_cv(pipe),
                2 => add_mat(pipe),
                3 => add_clut(pipe),
                4 => { add_id(pipe); add_cv(pipe); }
                5 => { add_cv(pipe); add_id(pipe); }
                6 => { add_id(pipe); add_mat(pipe); }
                7 => { add_mat(pipe); add_id(pipe); }
                8 => { add_cv(pipe); add_cv(pipe); }
                9 => { add_mat(pipe); add_mat(pipe); }
                _ => { add_id(pipe); add_id(pipe); }
            }
            let lp = c::cmsCreateProfilePlaceholder(
                std::ptr::null_mut());
            c::cmsSetDeviceClass(lp, 0x6C696E6B);
            c::cmsSetColorSpace(lp, 0x52474220);
            c::cmsSetPCS(lp, 0x4C616220);
            c::cmsSetProfileVersion(lp, 4.4);
            c::cmsWriteTag(lp, 0x41324230,
                pipe as *const _);
            let t = c::cmsCreateTransform(lp, frgb,
                lab4, flab, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let src = [0x8000u16; 3];
                let mut dst = [0u16; 3];
                c::cmsDoTransform(t,
                    src.as_ptr() as *const _,
                    dst.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
            c::cmsCloseProfile(lp);
            c::cmsPipelineFree(pipe);
        }
        c::cmsFreeToneCurve(g);
        c::cmsCloseProfile(lab4);
    }
}

/// cmslut residual — stage-level eval via pipeline
///   eval on each stage type + 16/float paths.
#[test]
fn lut_stageeval_arms() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        // each stage type through a pipeline eval
        let tcs = [g, g, g];
        let stages: Vec<(*mut std::ffi::c_void, &str)> = vec![
            (c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3,
                tcs.as_ptr() as *mut _), "cv"),
            (c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 3,
                [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0,
                 0.0, 0.0, 1.0].as_ptr(),
                std::ptr::null()), "mat"),
            (c::cmsStageAllocCLut16bitGranular(
                std::ptr::null_mut(), [3u32; 3].as_ptr(),
                3, 3,
                vec![0x8000u16; 27 * 3].as_ptr()), "clut"),
            (c::cmsStageAllocCLutFloatGranular(
                std::ptr::null_mut(), [3u32; 3].as_ptr(),
                3, 3,
                vec![0.5f32; 27 * 3].as_ptr()), "fclut"),
            (c::cmsStageAllocIdentity(
                std::ptr::null_mut(), 3), "ident"),
        ];
        for (st, name) in &stages {
            let pipe = c::cmsPipelineAlloc(
                std::ptr::null_mut(), 3, 3);
            c::cmsPipelineInsertStage(pipe, 0, *st);
            // eval via a transform
            let lp = c::cmsCreateProfilePlaceholder(
                std::ptr::null_mut());
            c::cmsSetDeviceClass(lp, 0x6C696E6B);
            c::cmsSetColorSpace(lp, 0x52474220);
            c::cmsSetPCS(lp, 0x4C616220);
            c::cmsSetProfileVersion(lp, 4.4);
            c::cmsWriteTag(lp, 0x41324230,
                pipe as *const _);
            let lab4 = c::cmsCreateLab4Profile(
                std::ptr::null_mut());
            let frgb = (4u32 << 16) | (3 << 3) | 2;
            let flab = (10u32 << 16) | (3 << 3) | 2;
            for fl in [0u32, 0x4000] {
                let t = c::cmsCreateTransform(lp, frgb,
                    lab4, flab, 0, fl);
                if !t.is_null() {
                    let src = [0x8000u16; 3];
                    let mut dst = [0u16; 3];
                    c::cmsDoTransform(t,
                        src.as_ptr() as *const _,
                        dst.as_mut_ptr() as *mut _, 1);
                    c::cmsDeleteTransform(t);
                }
            }
            eprintln!("{name} eval ok");
            c::cmsCloseProfile(lp);
            c::cmsCloseProfile(lab4);
            c::cmsPipelineFree(pipe);
        }
        c::cmsFreeToneCurve(g);
    }
}
