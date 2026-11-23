//! Transform parity across intents + formats + a non-sRGB profile.
//! Each case runs the same input through C's `cmsCreateTransform` and
//! Rust's `create_extended_transform`, comparing the raw output bytes.

use lcms::api::{Format, flags};
use lcms::signatures::colorspace;
use lcms::virt;
use lcms::xform;
use lcms_cref as c;

const TYPE_RGB_8: u32 = 4 << 16 | 3 << 3 | 1;
const TYPE_BGR_8: u32 = 4 << 16 | 3 << 3 | 1 | 1 << 10;
const TYPE_RGB_16: u32 = 4 << 16 | 3 << 3 | 2;
const TYPE_LAB_16: u32 = 10 << 16 | 3 << 3 | 2;

#[test]
fn rgba_float_clut_copies_alpha() {
    check_float_clut_fusion(
        colorspace::SigRgbData,
        Format::RGBA_FLT,
        3,
        flags::COPY_ALPHA,
    );
}

#[test]
fn rgba_float_clut_leaves_alpha_untouched() {
    check_float_clut_fusion(colorspace::SigRgbData, Format::RGBA_FLT, 3, 0);
}

#[test]
fn cmyk_float_clut_uses_ink_percentages() {
    check_float_clut_fusion(colorspace::SigCmykData, Format::CMYK_FLT, 4, 0);
}

fn check_float_clut_fusion(space: u32, format: Format, channels: u32, flags: u32) {
    use lcms::{
        intrp::{InterpTable, compute_interp_params},
        lut::{StageData, StageLoc, pipeline_alloc, stage_alloc_clut16bit},
        opt::{ClutOnly, OptData},
        profile::{cms_write_tag, create_profile_placeholder},
        signatures::{profileclass, tag},
        tagtypes::TagObject,
    };

    const WIDTH: u32 = 2;
    const HEIGHT: u32 = 2;
    const IN_WORDS: usize = 12;
    const OUT_WORDS: usize = 16;
    let rows: [[f32; IN_WORDS]; HEIGHT as usize] = [
        [
            0.7, 0.4, 0.2, 0.375, 0.0, 1.0, 0.6, -0.0, 99.0, 99.0, 99.0, 99.0,
        ],
        [
            0.23, 0.08, 0.9, 0.875, 1.0, 0.0, 0.0, 0.125, 99.0, 99.0, 99.0, 99.0,
        ],
    ];
    let samples: Vec<u16> = (0..1 << channels)
        .flat_map(|corner| {
            (0..channels).map(move |channel| {
                let high = corner & (1 << (channels - channel - 1)) != 0;
                if high {
                    48_000 - channel as u16 * 2000
                } else {
                    7000 + channel as u16 * 1000
                }
            })
        })
        .collect();
    let stage = stage_alloc_clut16bit(None, 2, channels, channels, Some(&samples)).unwrap();
    let StageData::CLut(clut) = stage.data.clone() else {
        panic!("expected CLUT");
    };
    let mut pipeline = pipeline_alloc(None, channels, channels).unwrap();
    assert!(pipeline.insert_stage(StageLoc::AtEnd, stage));
    let mut profile = create_profile_placeholder(None);
    profile.device_class = profileclass::SigLinkClass;
    profile.color_space = space;
    profile.pcs = space;
    assert!(cms_write_tag(
        &mut profile,
        tag::SigAToB0Tag,
        Some(TagObject::Lut16(pipeline))
    ));
    let mut input = rows;
    if channels == 4 {
        for row in &mut input {
            for ink in &mut row[..WIDTH as usize * 4] {
                *ink *= 100.0;
            }
        }
    }

    let c_profile = unsafe {
        let handle = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        assert!(!handle.is_null());
        c::cmsSetDeviceClass(handle, profileclass::SigLinkClass);
        c::cmsSetColorSpace(handle, space);
        c::cmsSetPCS(handle, space);
        let pipeline = c::cmsPipelineAlloc(std::ptr::null_mut(), channels, channels);
        assert!(!pipeline.is_null());
        let stage = c::cmsStageAllocCLut16bit(
            std::ptr::null_mut(),
            2,
            channels,
            channels,
            samples.as_ptr(),
        );
        assert!(!stage.is_null());
        assert_ne!(c::cmsPipelineInsertStage(pipeline, 1, stage), 0);
        assert_ne!(c::cmsWriteTag(handle, tag::SigAToB0Tag, pipeline), 0);
        c::cmsPipelineFree(pipeline);
        handle
    };
    let mut transform = xform::create_extended_transform(
        None,
        &mut [profile],
        &mut [false],
        &[1],
        &[1.0],
        None,
        0,
        format.code(),
        format.code(),
        flags,
    )
    .unwrap();
    let mut expected = [f32::from_bits(0x3f37_19a1); OUT_WORDS * HEIGHT as usize];
    let mut actual = expected;
    unsafe {
        let ct = c::cmsCreateTransform(
            c_profile,
            format.code(),
            std::ptr::null_mut(),
            format.code(),
            1,
            flags,
        );
        assert!(!ct.is_null());
        c::cmsDoTransformLineStride(
            ct,
            input.as_ptr().cast(),
            expected.as_mut_ptr().cast(),
            WIDTH,
            HEIGHT,
            (IN_WORDS * 4) as u32,
            (OUT_WORDS * 4) as u32,
            0,
            0,
        );
        c::cmsDeleteTransform(ct);
    }
    if format == Format::RGBA_FLT {
        for (src_row, dst_row) in input.iter().zip(expected.chunks_exact(OUT_WORDS)) {
            for alpha in (3..WIDTH as usize * 4).step_by(4) {
                let want = if flags & flags::COPY_ALPHA != 0 {
                    src_row[alpha]
                } else {
                    f32::from_bits(0x3f37_19a1)
                };
                assert_eq!(dst_row[alpha].to_bits(), want.to_bits());
            }
        }
    }
    transform.do_transform_line_stride(
        bytemuck::cast_slice(&input),
        bytemuck::cast_slice_mut(&mut actual),
        WIDTH,
        HEIGHT,
        (IN_WORDS * 4) as u32,
        (OUT_WORDS * 4) as u32,
        0,
        0,
    );
    assert_eq!(
        actual.map(f32::to_bits),
        expected.map(f32::to_bits),
        "{format:?} stock flags={flags:#x}"
    );

    // The stock optimizer does not resample float formats. Attach the
    // same CLUT as an optimized table to exercise the fused worker.
    let lut = transform.plan.lut.as_mut().unwrap();
    assert!(lut.opt_data.is_none());
    lut.opt_data = Some(OptData::ClutOnly(Box::new(ClutOnly {
        clut: clut.as_ref().clone(),
        params16: compute_interp_params(
            None,
            2,
            channels,
            channels,
            InterpTable::U16Rc(samples.clone().into()),
            0,
        )
        .unwrap(),
        params_f: None,
    })));
    actual.fill(f32::from_bits(0x3f37_19a1));
    transform.do_transform_line_stride(
        bytemuck::cast_slice(&input),
        bytemuck::cast_slice_mut(&mut actual),
        WIDTH,
        HEIGHT,
        (IN_WORDS * 4) as u32,
        (OUT_WORDS * 4) as u32,
        0,
        0,
    );
    assert_eq!(
        actual.map(f32::to_bits),
        expected.map(f32::to_bits),
        "{format:?} fused flags={flags:#x}"
    );

    unsafe { c::cmsCloseProfile(c_profile) };
}

#[test]
fn gray16_curve_transform_honors_output_layout() {
    use lcms::api::{ColorSpace, Intent, Plan, Profile, ToneCurve};
    let profile =
        Profile::linearization_device_link(ColorSpace::Gray, &[ToneCurve::gamma(2.2).unwrap()])
            .unwrap();
    let c_profile = unsafe {
        let curve = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let handle =
            c::cmsCreateLinearizationDeviceLink(colorspace::SigGrayData, [curve.cast()].as_ptr());
        c::cmsFreeToneCurve(curve);
        assert!(!handle.is_null());
        handle
    };
    let input = [0u16, 12_345, 32_768, u16::MAX];
    for format in [
        Format::GRAY_8,
        Format::GRAY_16,
        Format::from_u32(Format::GRAY_16.code() | 1 << 7),
        Format::from_u32(Format::GRAY_16.code() | 1 << 7).planar(),
        Format::from_u32(Format::GRAY_16.code() | 1 << 11),
        Format::from_u32(Format::GRAY_16.code() | 1 << 13),
    ] {
        let plan = Plan::new_multiprofile(
            &[&profile],
            Format::GRAY_16,
            format,
            Intent::RelativeColorimetric,
            flags::NO_CACHE,
        )
        .unwrap();
        // Extra space is sentinel-filled to catch stores outside the packed result.
        let mut expected = [0xaca5u16; 8];
        let mut actual = expected;
        unsafe {
            let ct = c::cmsCreateTransform(
                c_profile,
                Format::GRAY_16.code(),
                std::ptr::null_mut(),
                format.code(),
                1,
                flags::NO_CACHE,
            );
            assert!(!ct.is_null());
            c::cmsDoTransform(
                ct,
                input.as_ptr().cast(),
                expected.as_mut_ptr().cast(),
                input.len() as u32,
            );
            c::cmsDeleteTransform(ct);
        }
        plan.apply(
            &mut plan.scratch(),
            bytemuck::cast_slice(&input),
            bytemuck::cast_slice_mut(&mut actual),
        );
        assert_eq!(actual, expected, "{format:?}");
    }
    unsafe { c::cmsCloseProfile(c_profile) };
}

#[test]
fn gray_clut_transform_honors_lab_v2_encoding() {
    use lcms::api::{D50, Intent, Plan, Profile, ToneCurve};

    let gray = Profile::gray_curve(&D50, &ToneCurve::gamma(2.2).unwrap()).unwrap();
    let lab = Profile::lab4();
    let (c_gray, c_lab) = unsafe {
        let curve = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(c::cmsD50_xyY().cast(), curve);
        c::cmsFreeToneCurve(curve);
        let lab = c::cmsCreateLab4Profile(std::ptr::null());
        assert!(!gray.is_null() && !lab.is_null());
        (gray, lab)
    };
    let samples = [0u16, 691, 12_345, 32_768, 54_598, u16::MAX];
    let input8 = samples.map(|v| (v >> 8) as u8);
    let flags = flags::FORCE_CLUT | flags::NO_CACHE;
    for (from, to, input) in [
        (Format::GRAY_8, Format::LAB_V2_8, input8.as_slice()),
        (
            Format::GRAY_16,
            Format::LAB_V2_16,
            bytemuck::cast_slice(&samples),
        ),
    ] {
        let plan = Plan::build(&gray, &lab, from, to, Intent::RelativeColorimetric, flags).unwrap();
        let mut expected = [0u16; 18];
        let mut actual = expected;
        unsafe {
            let ct = c::cmsCreateTransform(c_gray, from.code(), c_lab, to.code(), 1, flags);
            assert!(!ct.is_null());
            c::cmsDoTransform(
                ct,
                input.as_ptr().cast(),
                expected.as_mut_ptr().cast(),
                samples.len() as u32,
            );
            c::cmsDeleteTransform(ct);
        }
        plan.apply(
            &mut plan.scratch(),
            input,
            bytemuck::cast_slice_mut(&mut actual),
        );
        assert_eq!(actual, expected, "{from:?} -> {to:?}");
    }
    unsafe {
        c::cmsCloseProfile(c_gray);
        c::cmsCloseProfile(c_lab);
    }
}

fn run_both(
    in_fmt: u32,
    out_fmt: u32,
    intent: u32,
    inp_bytes: usize,
    out_bytes: usize,
    n_pixels: usize,
) -> (Vec<u8>, Vec<u8>) {
    // C side
    let mut c_out = vec![0u8; n_pixels * out_bytes];
    unsafe {
        let prof = c::cmsCreate_sRGBProfile();
        let t = c::cmsCreateTransform(prof, in_fmt, prof, out_fmt, intent, 0);
        assert!(!t.is_null(), "C create_transform failed");
        let inp = vec![0u8; n_pixels * inp_bytes];
        c::cmsDoTransform(
            t,
            inp.as_ptr() as *const _,
            c_out.as_mut_ptr() as *mut _,
            n_pixels as u32,
        );
        c::cmsCloseProfile(prof);
    }
    // Rust side
    let mut rust_out = vec![0u8; n_pixels * out_bytes];
    let p1 = virt::create_srgb_profile(None).expect("srgb in");
    let p2 = virt::create_srgb_profile(None).expect("srgb out");
    let mut profs = vec![p1, p2];
    let mut bpc = [false, false];
    let intents = [intent, intent];
    let adapt = [1.0f64, 1.0];
    let mut t = xform::create_extended_transform(
        None, &mut profs, &mut bpc, &intents, &adapt, None, 0, in_fmt, out_fmt, 0,
    )
    .expect("rust create_extended_transform failed");
    let inp = vec![0u8; n_pixels * inp_bytes];
    t.do_transform(&inp, &mut rust_out, n_pixels as u32);
    (c_out, rust_out)
}

#[test]
fn srgb_all_intents_rgb8() {
    for intent in [0u32, 1, 2, 3] {
        let (c, r) = run_both(TYPE_RGB_8, TYPE_RGB_8, intent, 3, 3, 85);
        assert_eq!(c, r, "intent {intent} rgb8 mismatch");
    }
}

#[test]
fn srgb_16bit() {
    let (c, r) = run_both(TYPE_RGB_16, TYPE_RGB_16, 0, 6, 6, 85);
    assert_eq!(c, r, "rgb16 mismatch");
}

#[test]
fn srgb_bgr8() {
    let (c, r) = run_both(TYPE_BGR_8, TYPE_BGR_8, 0, 3, 3, 85);
    assert_eq!(c, r, "bgr8 mismatch");
}

#[test]
fn lab_to_srgb() {
    // Lab4 (abstract) → sRGB exercises the devicelink read path +
    // the output matrix-shaper
    let mut c_out = vec![0u8; 85 * 3];
    unsafe {
        let lab = c::cmsCreateLab4Profile(std::ptr::null());
        let srgb = c::cmsCreate_sRGBProfile();
        let t = c::cmsCreateTransform(lab, TYPE_LAB_16, srgb, TYPE_RGB_8, 0, 0);
        assert!(!t.is_null(), "C Lab→sRGB failed");
        let inp = vec![0u8; 85 * 6];
        c::cmsDoTransform(
            t,
            inp.as_ptr() as *const _,
            c_out.as_mut_ptr() as *mut _,
            85,
        );
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(srgb);
    }
    let mut rust_out = vec![0u8; 85 * 3];
    let p1 = virt::create_lab4_profile(None, None).expect("lab");
    let p2 = virt::create_srgb_profile(None).expect("srgb");
    let mut profs = vec![p1, p2];
    let mut bpc = [false, false];
    let intents = [0u32, 0];
    let adapt = [1.0f64, 1.0];
    let mut t = xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &adapt,
        None,
        0,
        TYPE_LAB_16,
        TYPE_RGB_8,
        0,
    )
    .expect("rust Lab→sRGB failed");
    let inp = vec![0u8; 85 * 6];
    t.do_transform(&inp, &mut rust_out, 85);
    assert_eq!(c_out, rust_out, "Lab→sRGB mismatch");
}

#[test]
fn srgb_to_xyz() {
    const TYPE_XYZ_DBL: u32 = 1 << 22 | 9 << 16 | 3 << 3;
    let mut c_out = vec![0u8; 85 * 24];
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let xyz = c::cmsCreateXYZProfile();
        let t = c::cmsCreateTransform(srgb, TYPE_RGB_8, xyz, TYPE_XYZ_DBL, 0, 0);
        assert!(!t.is_null(), "C sRGB→XYZ failed");
        let inp = vec![0u8; 85 * 3];
        c::cmsDoTransform(
            t,
            inp.as_ptr() as *const _,
            c_out.as_mut_ptr() as *mut _,
            85,
        );
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(xyz);
    }
    let mut rust_out = vec![0u8; 85 * 24];
    let p1 = virt::create_srgb_profile(None).expect("srgb");
    let p2 = virt::create_xyz_profile(None).expect("xyz");
    let mut profs = vec![p1, p2];
    let mut bpc = [false, false];
    let intents = [0u32, 0];
    let adapt = [1.0f64, 1.0];
    let mut t = xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &adapt,
        None,
        0,
        TYPE_RGB_8,
        TYPE_XYZ_DBL,
        0,
    )
    .expect("rust sRGB→XYZ failed");
    let inp = vec![0u8; 85 * 3];
    t.do_transform(&inp, &mut rust_out, 85);
    assert_eq!(c_out, rust_out, "sRGB→XYZ mismatch");
}

#[test]
fn lab2_to_srgb() {
    const TYPE_LAB_8: u32 = 10 << 16 | 3 << 3 | 1;
    let mut c_out = vec![0u8; 85 * 3];
    unsafe {
        let lab = c::cmsCreateLab2Profile(std::ptr::null());
        let srgb = c::cmsCreate_sRGBProfile();
        let t = c::cmsCreateTransform(lab, TYPE_LAB_8, srgb, TYPE_RGB_8, 0, 0);
        assert!(!t.is_null(), "C Lab2→sRGB failed");
        let inp = vec![0u8; 85 * 3];
        c::cmsDoTransform(
            t,
            inp.as_ptr() as *const _,
            c_out.as_mut_ptr() as *mut _,
            85,
        );
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(srgb);
    }
    let mut rust_out = vec![0u8; 85 * 3];
    let p1 = virt::create_lab2_profile(None, None).expect("lab2");
    let p2 = virt::create_srgb_profile(None).expect("srgb");
    let mut profs = vec![p1, p2];
    let mut bpc = [false, false];
    let intents = [0u32, 0];
    let adapt = [1.0f64, 1.0];
    let mut t = xform::create_extended_transform(
        None, &mut profs, &mut bpc, &intents, &adapt, None, 0, TYPE_LAB_8, TYPE_RGB_8, 0,
    )
    .expect("rust Lab2→sRGB failed");
    let inp = vec![0u8; 85 * 3];
    t.do_transform(&inp, &mut rust_out, 85);
    assert_eq!(c_out, rust_out, "Lab2→sRGB mismatch");
}

#[test]
fn srgb_forced_clut() {
    // cmsFLAGS_FORCE_CLUT exercises OptimizeByResampling + PrelinEval
    // paths that the matrix-shaper shortcut skips
    const FLAGS_FORCE_CLUT: u32 = 0x0002;
    let mut c_out = vec![0u8; 85 * 3];
    unsafe {
        let prof = c::cmsCreate_sRGBProfile();
        let t = c::cmsCreateTransform(prof, TYPE_RGB_8, prof, TYPE_RGB_8, 0, FLAGS_FORCE_CLUT);
        assert!(!t.is_null(), "C sRGB FORCED_CLUT failed");
        let inp = vec![0u8; 85 * 3];
        c::cmsDoTransform(
            t,
            inp.as_ptr() as *const _,
            c_out.as_mut_ptr() as *mut _,
            85,
        );
        c::cmsCloseProfile(prof);
    }
    let mut rust_out = vec![0u8; 85 * 3];
    let p1 = virt::create_srgb_profile(None).expect("srgb");
    let p2 = virt::create_srgb_profile(None).expect("srgb");
    let mut profs = vec![p1, p2];
    let mut bpc = [false, false];
    let intents = [0u32, 0];
    let adapt = [1.0f64, 1.0];
    let mut t = xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &adapt,
        None,
        0,
        TYPE_RGB_8,
        TYPE_RGB_8,
        FLAGS_FORCE_CLUT,
    )
    .expect("rust sRGB FORCED_CLUT failed");
    let inp = vec![0u8; 85 * 3];
    t.do_transform(&inp, &mut rust_out, 85);
    assert_eq!(c_out, rust_out, "sRGB FORCED_CLUT mismatch");
}
