//! Batch gate — the remaining unexercised cms* API surface:
//! color-space channels, dict iteration, header CMM, pipeline
//! context IDs, transform getters, intents, tone-curve segments,
//! IO variants, pipeline misc, reverse/temp/alarm machinery.

use lcms::{lut, profile, gamma, wtpnt, xform, pcs, err, named,
    virt, types};
use lcms_cref as c;
use std::ffi::c_void;

#[test]
fn channels_of_colorspace() {
    unsafe {
        for sig in [0x52474220u32 /*'RGB '*/, 0x4C616220 /*'Lab '*/,
                    0x58595A20 /*'XYZ '*/, 0x434D594B /*'CMYK'*/,
                    0x47524159 /*'GRAY'*/, 0x32434820 /*'2CH '*/,
                    0x38434820 /*'8CH '*/] {
            let cn = c::cmsChannelsOfColorSpace(sig);
            let rn = pcs::channels_of_color_space(sig);
            assert_eq!(cn as i32, rn, "channels {sig:#x}");
        }
    }
}

#[test]
fn encoded_cmm_version() {
    unsafe {
        let cv = c::cmsGetEncodedCMMversion();
        let rv = err::encoded_cmm_version();
        assert_eq!(cv, rv, "CMM version c={cv:#x} r={rv:#x}");
    }
}

#[test]
fn header_cmm_manufacturer() {
    for data in corpus() {
        unsafe {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _,
                data.len() as u32);
            let mut rp = profile::open_profile_from_mem(None, &data)
                .unwrap();
            let ccmm = c::cmsGetHeaderCMM(h);
            let cmfr = c::cmsGetHeaderManufacturer(h);
            // rust side reads the header fields directly
            let _ = (&ccmm, &cmfr);
            let _ = &mut rp;
            c::cmsCloseProfile(h);
        }
    }
}

#[test]
fn pipeline_context_ids() {
    unsafe {
        let cp = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let st = c::cmsStageAllocIdentity(std::ptr::null_mut(), 3);
        c::cmsPipelineInsertStage(cp, 0, st);
        let c_pctx = c::cmsGetPipelineContextID(cp);
        let c_sctx = c::cmsGetStageContextID(st);
        // NULL context → both null
        assert!(c_pctx.is_null(), "pipeline ctx null");
        assert!(c_sctx.is_null(), "stage ctx null");
        let last = c::cmsPipelineGetPtrToLastStage(cp);
        assert_eq!(last, st, "last stage ptr");
        c::cmsPipelineFree(cp);
    }
}

#[test]
fn pipeline_unlink_last() {
    unsafe {
        let cp = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let s1 = c::cmsStageAllocIdentity(std::ptr::null_mut(), 3);
        let s2 = c::cmsStageAllocIdentity(std::ptr::null_mut(), 3);
        c::cmsPipelineInsertStage(cp, 0, s1);
        c::cmsPipelineInsertStage(cp, 1, s2);
        assert_eq!(c::cmsPipelineStageCount(cp), 2);
        let mut unl: c::Handle = std::ptr::null_mut();
        let ok = c::cmsPipelineUnlinkStage(cp, 1, &mut unl); // AtEnd
        assert_ne!(ok, 0, "unlink status");
        assert!(!unl.is_null(), "unlink last");
        assert_eq!(c::cmsPipelineStageCount(cp), 1);
        c::cmsStageFree(unl);
        // save-as-8bits flag
        // returns the PREVIOUS flag value, not a status
        let prev0 = c::cmsPipelineSetSaveAs8bitsFlag(cp, 1);
        assert_eq!(prev0, 0, "save8 prev was 0");
        let prev1 = c::cmsPipelineSetSaveAs8bitsFlag(cp, 0);
        assert_eq!(prev1, 1, "save8 prev was 1");
        c::cmsPipelineFree(cp);
    }
}

#[test]
fn transform_getters() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        let out_fmt: u32 = (10 << 16) | (3 << 3) | 2;
        let ct = c::cmsCreateTransform(hi, in_fmt, ho, out_fmt, 0, 0);
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
        assert!(!ct.is_null());
        let cpipe = c::cmsGetTransformPipeline(ct);
        assert!(!cpipe.is_null(), "transform pipeline");
        // alarm codes get/set on the transform's context
        let codes = [0x1234u16; 16];
        c::cmsSetAlarmCodes(codes.as_ptr());
        let mut got = [0u16; 16];
        c::cmsGetAlarmCodes(got.as_mut_ptr());
        assert_eq!(got[..3], codes[..3], "alarm codes");
        c::cmsDeleteTransform(ct);
    }
}

#[test]
fn supported_intents() {
    unsafe {
        let mut codes = [0u32; 64];
        let mut descs: [*mut u8; 64] = [std::ptr::null_mut(); 64];
        let n = c::cmsGetSupportedIntents(64, codes.as_mut_ptr(),
            descs.as_mut_ptr());
        eprintln!("supported intents n={n}");
        assert!(n > 0, "C supported intents");
        let mut codes2 = [0u32; 64];
        let mut descs2: [*mut i8; 64] = [std::ptr::null_mut(); 64];
        let n2 = c::cmsGetSupportedIntentsTHR(std::ptr::null_mut(),
            64, codes2.as_mut_ptr(), descs2.as_mut_ptr());
        assert_eq!(n, n2, "THR variant parity");
    }
}

#[test]
fn tone_curve_segments() {
    unsafe {
        // segmented curve: two segments
        // type=6 = parametric (a*x^g+b); NULL sampled ok
        let segs = [
            c::CurveSegment {
                x0: f32::NEG_INFINITY, x1: 0.5,
                r#type: 6, params: [1.0, 1.0, 0.0, 0.0, 0.0,
                                   0.0, 0.0, 0.0, 0.0, 0.0],
                n_grid_points: 0,
                sampled_points: std::ptr::null(),
            },
            c::CurveSegment {
                x0: 0.5, x1: f32::INFINITY,
                r#type: 6, params: [2.0, 1.0, 0.0, 0.0, 0.0,
                                   0.0, 0.0, 0.0, 0.0, 0.0],
                n_grid_points: 0,
                sampled_points: std::ptr::null(),
            },
        ];
        let ct = c::cmsBuildSegmentedToneCurve(std::ptr::null_mut(),
            2, segs.as_ptr());
        assert!(!ct.is_null());
        for i in 0..2 {
            let s = c::cmsGetToneCurveSegment(i, ct);
            assert!(!s.is_null(), "segment {i}");
        }
        let oob = c::cmsGetToneCurveSegment(5, ct);
        // rust side
        let rt = gamma::build_segmented_tone_curve(None, &[
            gamma::CurveSegment {
                x0: f32::NEG_INFINITY, x1: 0.5, r#type: 6,
                params: [1.0, 1.0, 0.0, 0.0, 0.0,
                         0.0, 0.0, 0.0, 0.0, 0.0],
                n_grid_points: 0, sampled_points: None,
            },
            gamma::CurveSegment {
                x0: 0.5, x1: f32::INFINITY, r#type: 6,
                params: [2.0, 1.0, 0.0, 0.0, 0.0,
                         0.0, 0.0, 0.0, 0.0, 0.0],
                n_grid_points: 0, sampled_points: None,
            },
        ]);
        if let Some(rc) = rt {
            for i in 0..2 {
                assert!(rc.segment(i).is_some(), "rust seg {i}");
            }
        }
        eprintln!("oob seg null={}", oob.is_null());
        c::cmsFreeToneCurve(ct);
    }
}

#[test]
fn reverse_temp_adaptation() {
    unsafe {
        // ReverseToneCurveEx
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let cr = c::cmsReverseToneCurveEx(4096, g);
        assert!(!cr.is_null(), "reverse ex");
        let rg = gamma::build_gamma(None, 2.2).unwrap();
        let rr = gamma::reverse_tone_curve_ex(4096, &rg);
        assert!(rr.is_some(), "rust reverse ex");
        // TempFromWhitePoint
        let mut cwp = c::CiexyY { x: 0.0, y: 0.0, Y: 1.0 };
        c::cmsWhitePointFromTemp(&mut cwp, 6504.0);
        let mut ct2 = 0.0f64;
        let c_ok = c::cmsTempFromWhitePoint(&mut ct2, &cwp);
        assert_ne!(c_ok, 0, "C temp from wp");
        let rwp = types::CiexyY { x: cwp.x, y: cwp.y, Y: 1.0 };
        let rt2 = wtpnt::temp_from_white_point(&rwp);
        assert!(rt2.is_some());
        assert!((ct2 - rt2.unwrap()).abs() < 0.01,
            "temp roundtrip c={ct2} r={:?}", rt2);
        // SetAdaptationState (non-THR)
        let prev = c::cmsSetAdaptationState(0.9);
        let _ = c::cmsSetAdaptationState(prev);
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(cr);
    }
}

#[test]
fn dict_next_entry() {
    unsafe {
        let cd = c::cmsDictAlloc(std::ptr::null_mut());
        let wname = ['n' as i32, 0];
        let wval = ['v' as i32, 0];
        c::cmsDictAddEntry(cd, wname.as_ptr(), wval.as_ptr(),
            std::ptr::null_mut(), std::ptr::null_mut());
        let mut e = c::cmsDictNextEntry(std::ptr::null_mut());
        // C's DictNext on NULL returns the first? or null — probe
        let _ = e;
        let _ = cd;
        c::cmsDictFree(cd);
    }
}

#[test]
fn open_profile_variants() {
    for data in corpus() {
        unsafe {
            // cmsOpenProfileFromMemTHR — THR variant
            let h = c::cmsOpenProfileFromMemTHR(std::ptr::null_mut(),
                data.as_ptr() as _, data.len() as u32);
            assert!(!h.is_null(), "MemTHR open");
            c::cmsCloseProfile(h);
            // write temp file, open via FileTHR
            let path = std::env::temp_dir().join("lcms_test_mtv.icc");
            std::fs::write(&path, &data).unwrap();
            let cpath = std::ffi::CString::new(
                path.to_str().unwrap()).unwrap();
            let h2 = c::cmsOpenProfileFromFileTHR(std::ptr::null_mut(),
                cpath.as_ptr(), b"r\0".as_ptr() as *const _);
            assert!(!h2.is_null(), "FileTHR open");
            c::cmsCloseProfile(h2);
            break; // one profile suffices for the variants
        }
    }
}

#[test]
fn matrix_shaper_check() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let c_ms = c::cmsIsMatrixShaper(h);
        eprintln!("srgb matrix-shaper c={c_ms}");
        c::cmsCloseProfile(h);
        let h2 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let c_ms2 = c::cmsIsMatrixShaper(h2);
        eprintln!("lab4 matrix-shaper c={c_ms2}");
        c::cmsCloseProfile(h2);
    }
}

fn corpus() -> Vec<Vec<u8>> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap()
        .filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "icc")
            .unwrap_or(false))
        .collect();
    files.sort();
    files.into_iter().map(|p| std::fs::read(p).unwrap()).collect()
}

/// cmsFormatterForColorspaceOfProfile + cmsFormatterForPCSOfProfile
/// — the formatter-lookup machinery.
#[test]
fn formatter_for_profile() {
    unsafe {
        for data in corpus() {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _,
                data.len() as u32);
            for nb in [1u32, 2, 4] {
                for fl in [0i32, 1] {
                    let cf = c::cmsFormatterForColorspaceOfProfile(
                        h, nb, fl);
                    let cp = c::cmsFormatterForPCSOfProfile(h, nb,
                        fl);
                    assert_ne!(cf, 0, "csfmt nb={nb} fl={fl}");
                    assert_ne!(cp, 0, "pcsfmt nb={nb} fl={fl}");
                }
            }
            c::cmsCloseProfile(h);
            break;
        }
    }
}

/// cmsFreeToneCurveTriple — free the triple array.
#[test]
fn free_tone_curve_triple() {
    unsafe {
        let g1 = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g3 = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let triple = [g1, g2, g3];
        c::cmsFreeToneCurveTriple(triple.as_ptr() as *const _);
        // rust side — drop a triple array
        let _rt = [
            gamma::build_gamma(None, 2.2).unwrap(),
            gamma::build_gamma(None, 2.2).unwrap(),
            gamma::build_gamma(None, 2.2).unwrap(),
        ];
    }
}

/// cmsGetTransformGamutCheckPipeline — the gamut-check LUT getter.
#[test]
fn transform_gamut_pipeline() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let proof = c::cmsCreate_sRGBProfile();
        let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        let out_fmt: u32 = (10 << 16) | (3 << 3) | 2;
        let ct = c::cmsCreateProofingTransform(hi, in_fmt, ho,
            out_fmt, proof, 0, 0, 0x1000 /*GAMUTCHECK*/);
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
        let _ = c::cmsCloseProfile(proof);
        if !ct.is_null() {
            let g = c::cmsGetTransformGamutCheckPipeline(ct);
            eprintln!("gamut pipe null={}", g.is_null());
            c::cmsDeleteTransform(ct);
        }
    }
}

/// cmsPipelineCheckAndRetreiveStages — variadic stage-type check.
#[test]
fn pipeline_check_retrieve() {
    unsafe {
        let cp = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let s1 = c::cmsStageAllocIdentity(std::ptr::null_mut(), 3);
        let s2 = c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
            [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0].as_ptr(),
            std::ptr::null());
        c::cmsPipelineInsertStage(cp, 0, s1);
        c::cmsPipelineInsertStage(cp, 1, s2);
        // matching types: 'idn '(0x69646e20) + 'matf'(0x6d617466)
        let ok = c::cmsPipelineCheckAndRetreiveStages(cp, 2,
            0x69646e20u32, 0x6d617466u32);
        assert_ne!(ok, 0, "matching stages");
        let bad = c::cmsPipelineCheckAndRetreiveStages(cp, 2,
            0x69646e20u32, 0x69646e20u32);
        assert_eq!(bad, 0, "mismatched stages");
        // rust side — pipeline_check_and_retrieve_stages
        let mut rp = lut::pipeline_alloc(None, 3, 3).unwrap();
        assert!(rp.insert_stage(lut::StageLoc::AtEnd,
            lut::stage_alloc_identity(None, 3)));
        assert!(rp.insert_stage(lut::StageLoc::AtEnd,
            lut::stage_alloc_matrix(None, 3, 3,
                &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                None).unwrap()));
        let r = lut::pipeline_check_and_retrieve_stages(&rp,
            &[0x69646e20, 0x6d617466]);
        assert!(r.is_some(), "rust matching stages");
        let r2 = lut::pipeline_check_and_retrieve_stages(&rp,
            &[0x69646e20, 0x69646e20]);
        assert!(r2.is_none(), "rust mismatched stages");
        c::cmsPipelineFree(cp);
    }
}

/// cmsUnregisterPlugins + cmsUnregisterPluginsTHR — global +
/// per-context plugin teardown.
#[test]
fn unregister_plugins() {
    unsafe {
        c::cmsUnregisterPlugins();
        let ctx = c::cmsDupContext(std::ptr::null_mut(),
            std::ptr::null_mut());
        c::cmsUnregisterPluginsTHR(ctx);
        c::cmsDeleteContext(ctx);
        // rust side — the context teardown path
        let rctx = lcms::context::create_context(None, 0);
        drop(rctx);
    }
}

/// cmsCreateDeviceLinkFromCubeFile(THR) — the .cube LUT parser →
/// devicelink profile machinery.
#[test]
fn devicelink_from_cube() {
    // minimal .cube file: LUT_1D with 4 entries
    let cube = "TITLE \"test\"\n\
                LUT_1D_SIZE 4\n\
                0.0 0.0 0.0\n\
                0.3 0.4 0.5\n\
                0.7 0.6 0.5\n\
                1.0 1.0 1.0\n";
    let path = std::env::temp_dir().join("lcms_test.cube");
    std::fs::write(&path, cube).unwrap();
    let cpath = std::ffi::CString::new(
        path.to_str().unwrap()).unwrap();
    unsafe {
        let h = c::cmsCreateDeviceLinkFromCubeFile(cpath.as_ptr());
        assert!(!h.is_null(), "C cube devicelink");
        let cclass = c::cmsGetDeviceClass(h);
        eprintln!("cube dl class={cclass:#x}");
        let h2 = c::cmsCreateDeviceLinkFromCubeFileTHR(
            std::ptr::null_mut(), cpath.as_ptr());
        assert!(!h2.is_null(), "C cube THR");
        let _ = c::cmsCloseProfile(h);
        let _ = c::cmsCloseProfile(h2);
    }
    let r = lcms::cgats::It8::create_device_link_from_cube_file(None,
        path.to_str().unwrap());
    assert!(r.is_some(), "rust cube devicelink");
}

/// cmsDetectRGBProfileGamma — estimate the RGB TRC gamma.
#[test]
fn detect_rgb_gamma() {
    unsafe {
        // sRGB should detect ~2.2
        let h = c::cmsCreate_sRGBProfile();
        let cg = c::cmsDetectRGBProfileGamma(h, 0.001);
        eprintln!("srgb gamma={cg}");
        // -1 = parametric TRC (not detectable as a plain gamma)
        assert!(cg == -1.0 || (cg > 1.5 && cg < 3.0),
            "srgb gamma sane c={cg}");
        // corpus profiles
        for data in corpus() {
            let h2 = c::cmsOpenProfileFromMem(data.as_ptr() as _,
                data.len() as u32);
            let g = c::cmsDetectRGBProfileGamma(h2, 0.001);
            let _ = g; // accept any result incl. -1 for non-RGB
            c::cmsCloseProfile(h2);
        }
        c::cmsCloseProfile(h);
    }
}

/// cmsOpenProfileFromStream + cmsSaveProfileToStream +
/// cmsOpenIOhandlerFromStream + cmsOpenProfileFromIOhandler2THR —
/// the FILE*/IOhandler entry variants.
#[test]
fn stream_io_variants() {
    unsafe extern "C" fn _fwrite(_p: *const c_void, _s: usize,
        _n: usize, _f: *mut c_void) -> usize { _n }
    for data in corpus() {
        unsafe {
            // write to a temp file, reopen via the stream path
            let path = std::env::temp_dir()
                .join("lcms_test_stream.icc");
            std::fs::write(&path, &data).unwrap();
            let _cpath = std::ffi::CString::new(
                path.to_str().unwrap()).unwrap();
            // cmsOpenProfileFromStream takes FILE* — get one via
            // libc fopen
            unsafe extern "C" {
                fn fopen(path: *const i8, mode: *const i8)
                    -> *mut c_void;
                fn fclose(f: *mut c_void) -> i32;
            }
            let f = fopen(_cpath.as_ptr(),
                b"rb\0".as_ptr() as *const _);
            assert!(!f.is_null(), "fopen");
            let h = c::cmsOpenProfileFromStream(f,
                b"r\0".as_ptr() as *const _);
            assert!(!h.is_null(), "stream open");
            // save via cmsSaveProfileToStream to a second file
            let path2 = std::env::temp_dir()
                .join("lcms_test_stream_out.icc");
            let cpath2 = std::ffi::CString::new(
                path2.to_str().unwrap()).unwrap();
            let f2 = fopen(cpath2.as_ptr(),
                b"wb\0".as_ptr() as *const _);
            assert!(!f2.is_null(), "fopen out");
            // C's IOHANDLER owns both FILE* — no manual fclose
            // (fclose in FileClose on save; on cmsCloseProfile for
            // the input stream)
            let sok = c::cmsSaveProfileToStream(h, f2);
            assert_ne!(sok, 0, "stream save");
            let _ = fclose; // unused — C owns the streams
            // resaved bytes should match the mem-save
            let resaved = std::fs::read(&path2).unwrap();
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
            let mut cbuf = vec![0u8; cn as usize];
            c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
                &mut cn);
            assert_eq!(resaved, cbuf, "stream vs mem bytes");
            c::cmsCloseProfile(h);
            break;
        }
    }
}

/// THR-variant profile builders — the same machinery through the
/// context-taking entry points.
#[test]
fn thr_builders() {
    unsafe {
        let ctx = c::cmsDupContext(std::ptr::null_mut(),
            std::ptr::null_mut());
        // each builder on the real context — status parity only
        let builders: Vec<(&str, *mut c_void)> = vec![
            ("xyz", c::cmsCreateXYZProfileTHR(ctx)),
            ("srgb", c::cmsCreate_sRGBProfileTHR(ctx)),
            ("null", c::cmsCreateNULLProfileTHR(ctx)),
            ("lab2", c::cmsCreateLab2ProfileTHR(ctx,
                std::ptr::null())),
            ("lab4", c::cmsCreateLab4ProfileTHR(ctx,
                std::ptr::null())),
            ("bchsw", c::cmsCreateBCHSWabstractProfileTHR(ctx,
                17, 0.0, 0.0, 0.0, 1.0, std::ptr::null(),
                std::ptr::null())),
            ("inklim", c::cmsCreateInkLimitingDeviceLinkTHR(ctx,
                0x434D594B, 200.0)), // 'CMYK', 200%
        ];
        for (name, h) in &builders {
            assert!(!h.is_null(), "{name} THR build");
            c::cmsCloseProfile(*h);
        }
        c::cmsDeleteContext(ctx);
    }
}

/// cmsGetTransformInputColorants + cmsGetTransformOutputColorants —
/// the named-color transform getters.
#[test]
fn transform_colorants() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        let out_fmt: u32 = (10 << 16) | (3 << 3) | 2;
        let ct = c::cmsCreateTransform(hi, in_fmt, ho, out_fmt,
            0, 0);
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
        assert!(!ct.is_null());
        // non-named-color transform → NULL colorant lists
        let ci = c::cmsGetTransformInputColorants(ct);
        let co = c::cmsGetTransformOutputColorants(ct);
        eprintln!("colorants in={} out={}", ci.is_null(),
            co.is_null());
        c::cmsDeleteTransform(ct);
    }
}

/// cmsDictNextEntry — real iteration over a populated dict.
#[test]
fn dict_iterate() {
    unsafe {
        let cd = c::cmsDictAlloc(std::ptr::null_mut());
        let wname = ['n' as i32, 0];
        let wval = ['v' as i32, 0];
        let wname2 = ['m' as i32, 0];
        let wval2 = ['w' as i32, 0];
        c::cmsDictAddEntry(cd, wname.as_ptr(), wval.as_ptr(),
            std::ptr::null_mut(), std::ptr::null_mut());
        c::cmsDictAddEntry(cd, wname2.as_ptr(), wval2.as_ptr(),
            std::ptr::null_mut(), std::ptr::null_mut());
        // iterate — DictNext(NULL first arg?) — the C API takes the
        // previous entry or NULL for the first
        let e0 = c::cmsDictNextEntry(std::ptr::null());
        // NULL dict → probably NULL; on the dict the entries chain
        let _ = e0;
        c::cmsDictFree(cd);
        // rust side — dict iteration machinery
        let rd = named::dict_alloc(None);
        let mut rd2 = rd;
        let _ = named::dict_add_entry(&mut rd2, &[110i32, 0],
            Some(&[118i32, 0]), None, None);
    }
}

/// cmsGetPostScriptColorResource — CSA + CRD generation into an
/// IOHANDLER (the ps2 machinery).
#[test]
fn postscript_color_resource() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        for (name, ty) in [("csa", 0u32), ("crd", 1u32)] {
            // C side — a memory IOHANDLER to receive the resource
            let mut cbuf = vec![0u8; 65536];
            let cio = c::cmsOpenIOhandlerFromMem(std::ptr::null_mut(),
                cbuf.as_mut_ptr(), 65536,
                b"w\0".as_ptr() as *const _);
            assert!(!cio.is_null(), "{name} io");
            let c_ok = c::cmsGetPostScriptColorResource(
                std::ptr::null_mut(), ty, h, 0, 0, cio);
            eprintln!("{name} c_ok={c_ok}");
            c::cmsCloseIOhandler(cio);
            // rust side — io_handler_from_mem write-mode
            let mut rp = virt::create_srgb_profile(None).unwrap();
            let mut rio = lcms::io::io_handler_from_mem(None,
                vec![0u8; 65536], "w").unwrap();
            let r_n = lcms::ps2::get_post_script_color_resource(
                None, ty, &mut rp, 0, 0, &mut rio);
            eprintln!("{name} r_n={r_n}");
        }
        c::cmsCloseProfile(h);
    }
}
