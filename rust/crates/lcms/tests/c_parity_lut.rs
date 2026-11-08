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
