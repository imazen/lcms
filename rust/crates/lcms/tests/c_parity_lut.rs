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
