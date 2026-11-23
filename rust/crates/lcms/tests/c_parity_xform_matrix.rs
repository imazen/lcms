//! Transform parity across intents + formats + a non-sRGB profile.
//! Each case runs the same input through C's `cmsCreateTransform` and
//! Rust's `create_extended_transform`, comparing the raw output bytes.

use lcms::api::{Format, flags};
use lcms::signatures::colorspace;
use lcms::virt;
use lcms::xform;
use lcms_cref as c;

#[path = "support/throughput_profiles.rs"]
mod throughput_profiles;

const TYPE_RGB_8: u32 = 4 << 16 | 3 << 3 | 1;
const TYPE_BGR_8: u32 = 4 << 16 | 3 << 3 | 1 | 1 << 10;
const TYPE_RGB_16: u32 = 4 << 16 | 3 << 3 | 2;
const TYPE_LAB_16: u32 = 10 << 16 | 3 << 3 | 2;

const PACKED_WIDTH: usize = 259;
const PACKED_ROWS: usize = 3;

fn check_packed_route<T: bytemuck::Pod>(
    src_bytes: &[u8],
    src_format: Format,
    dst_format: Format,
    input: &[T],
    pad: T,
    flags: u32,
) {
    use lcms::api::{Intent, Plan, PlanOptions, Profile};
    let src = Profile::open(src_bytes).unwrap();
    let dst = Profile::srgb().save().unwrap();
    let plan = Plan::with_options(
        &src,
        &Profile::open(&dst).unwrap(),
        src_format,
        dst_format,
        &PlanOptions::new()
            .intent(Intent::RelativeColorimetric)
            .flags(flags),
    )
    .unwrap();
    let size = size_of::<T>();
    let src_stride = PACKED_WIDTH * src_format.pixel_size() + 4 * size;
    let dst_stride = PACKED_WIDTH * dst_format.pixel_size() + 5 * size;
    let mut expected = vec![pad; dst_stride / size * PACKED_ROWS];
    let mut actual = expected.clone();
    unsafe {
        let src = c::cmsOpenProfileFromMem(src_bytes.as_ptr().cast(), src_bytes.len() as u32);
        let dst = c::cmsOpenProfileFromMem(dst.as_ptr().cast(), dst.len() as u32);
        assert!(!src.is_null() && !dst.is_null());
        let transform =
            c::cmsCreateTransform(src, src_format.code(), dst, dst_format.code(), 1, flags);
        assert!(!transform.is_null());
        c::cmsDoTransformLineStride(
            transform,
            input.as_ptr().cast(),
            expected.as_mut_ptr().cast(),
            PACKED_WIDTH as u32,
            PACKED_ROWS as u32,
            src_stride as u32,
            dst_stride as u32,
            0,
            0,
        );
        c::cmsDeleteTransform(transform);
        c::cmsCloseProfile(src);
        c::cmsCloseProfile(dst);
    }
    plan.apply_strided(
        &mut plan.scratch(),
        bytemuck::cast_slice(input),
        src_stride,
        bytemuck::cast_slice_mut(&mut actual),
        dst_stride,
        PACKED_WIDTH,
        PACKED_ROWS,
    );
    for (i, (ours, oracle)) in bytemuck::cast_slice::<T, u8>(&actual)
        .chunks_exact(size)
        .zip(bytemuck::cast_slice::<T, u8>(&expected).chunks_exact(size))
        .enumerate()
    {
        assert_eq!(
            ours, oracle,
            "{src_format:?} -> {dst_format:?}, flags {flags:#x}, sample {i}"
        );
    }
}

fn packed_witness<T: Copy>(channels: usize, mut sample: impl FnMut(usize) -> T) -> Vec<T> {
    let row_samples = PACKED_WIDTH * channels + 4;
    (0..row_samples * PACKED_ROWS)
        .map(|i| {
            let px = i % row_samples / channels;
            let cell = i % row_samples % channels;
            match px {
                0 | 1 => sample(cell),            // repeated black, including the seeded cache
                2 | 3 => sample(channels + cell), // repeated white, including the final CLUT node
                _ => sample(2 * channels + i),
            }
        })
        .collect()
}

#[test]
fn packed_matrix_shaper8_matches_c() {
    for profile in [
        throughput_profiles::display_p3(),
        throughput_profiles::adobe_rgb(),
    ] {
        for (format, n) in [
            (Format::RGB_8, 3),
            (Format::BGR_8, 3),
            (Format::RGBA_8, 4),
            (Format::BGRA_8, 4),
        ] {
            let input = packed_witness(n, |i| match i {
                i if i < n => 0,
                i if i < 2 * n => 255,
                _ => (i * 131 + i / 7) as u8,
            });
            for flags in [0, flags::COPY_ALPHA] {
                check_packed_route(&profile, format, format, &input, 0xa7, flags);
            }
        }
    }
}

#[test]
fn packed_rgb_clut8_matches_c_at_endpoints_and_with_cache() {
    let profile = throughput_profiles::synthetic_rgb_icc();
    let input = packed_witness(3, |i| match i {
        0..3 => 0,
        3..6 => 255,
        _ => (i * 113 + i / 7) as u8,
    });
    for flags in [0, flags::NO_CACHE] {
        check_packed_route(&profile, Format::RGB_8, Format::RGB_8, &input, 0xa7, flags);
    }
}

#[test]
fn packed_cmyk_clut_matches_c_with_and_without_cache() {
    let profile = throughput_profiles::synthetic_cmyk_icc();
    let input8 = packed_witness(4, |i| match i {
        0..4 => 0,
        4..8 => 255,
        _ => (i * 43 + i / 13) as u8,
    });
    let input16 = packed_witness(4, |i| match i {
        0..4 => 0,
        4..8 => u16::MAX,
        _ => (i * 9973 + 12345) as u16,
    });
    for flags in [0, flags::NO_CACHE, flags::BLACK_POINT_COMPENSATION] {
        for format in [Format::RGB_8, Format::RGBA_8] {
            check_packed_route(&profile, Format::CMYK_8, format, &input8, 0xa7, flags);
        }
        for format in [Format::RGB_16, Format::RGBA_16] {
            check_packed_route(&profile, Format::CMYK_16, format, &input16, 0xa5a7, flags);
        }
    }
}

#[test]
fn packed_rgb_and_cmyk_float_profiles_match_c() {
    let pad = f32::from_bits(0x3f37_19a1);
    for profile in [
        throughput_profiles::display_p3(),
        throughput_profiles::adobe_rgb(),
        throughput_profiles::synthetic_rgb_icc(),
    ] {
        for (format, n) in [(Format::RGB_FLT, 3), (Format::RGBA_FLT, 4)] {
            let input = packed_witness(n, |i| match i {
                i if i < n => -0.0,
                i if i < n * 2 => 1.0,
                i if n == 4 && i % 4 == 3 => f32::from_bits(0x7fc1_a117),
                _ => ((i * 37 + 17) % 1009) as f32 / 864.0 - 0.075,
            });
            for flags in [0, flags::COPY_ALPHA] {
                check_packed_route(&profile, format, format, &input, pad, flags);
            }
        }
    }
    let profile = throughput_profiles::synthetic_cmyk_icc();
    let input = packed_witness(4, |i| match i {
        0..4 => -0.0,
        4..8 => 100.0,
        _ => ((i * 83 + 19) % 1009) as f32 / 10.0,
    });
    for flags in [0, flags::BLACK_POINT_COMPENSATION] {
        check_packed_route(
            &profile,
            Format::CMYK_FLT,
            Format::RGB_FLT,
            &input,
            pad,
            flags,
        );
    }
    let profile = lcms::api::Profile::srgb().save().unwrap();
    let input = packed_witness(3, |i| {
        [
            f32::from_bits(0x7f81_2345),
            f32::from_bits(0x7fc1_2345),
            f32::NEG_INFINITY,
            f32::INFINITY,
            -0.0,
            0.0,
        ][i % 6]
    });
    check_packed_route(&profile, Format::RGB_FLT, Format::RGB_FLT, &input, pad, 0);
}

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
