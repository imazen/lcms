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
    let dst = lcms::api::Profile::srgb().save().unwrap();
    check_packed_pair(src_bytes, src_format, &dst, dst_format, input, pad, flags);
}

fn check_packed_pair<T: bytemuck::Pod>(
    src_bytes: &[u8],
    src_format: Format,
    dst_bytes: &[u8],
    dst_format: Format,
    input: &[T],
    pad: T,
    flags: u32,
) {
    use lcms::api::{Intent, Plan, PlanOptions, Profile};
    let src = Profile::open(src_bytes).unwrap();
    let plan = Plan::with_options(
        &src,
        &Profile::open(dst_bytes).unwrap(),
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
        let dst = c::cmsOpenProfileFromMem(dst_bytes.as_ptr().cast(), dst_bytes.len() as u32);
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
fn packed_matrix_shaper8_matches_c_with_distinct_output_curves() {
    use lcms::api::{CiexyY, CiexyYTriple, D65, Profile, ToneCurve};

    let primary = |x, y| CiexyY { x, y, Y: 1.0 };
    let primaries = CiexyYTriple {
        Red: primary(0.64, 0.33),
        Green: primary(0.30, 0.60),
        Blue: primary(0.15, 0.06),
    };
    let [r, g, b] = [1.8, 2.2, 2.6].map(|gamma| ToneCurve::gamma(gamma).unwrap());
    let dst = Profile::rgb(&D65, &primaries, [&r, &g, &b])
        .unwrap()
        .save()
        .unwrap();
    let src = throughput_profiles::display_p3();
    let input = packed_witness(4, |i| ((i * 131 + i / 7) & 255) as u8);
    check_packed_pair(
        &src,
        Format::BGRA_8,
        &dst,
        Format::BGRA_8,
        &input,
        0xa7,
        flags::COPY_ALPHA,
    );
}

#[test]
fn sampled_float_matrix_shapers_preserve_alpha_and_extended_input() {
    use lcms::api::{BehaviorMode, Format, Intent, Plan, PlanOptions, Profile, flags};

    let width = 3839;
    let height = 3;
    let stride = (width + 2) * 4;
    let pad = -99.0f32;
    let mut input = vec![pad; stride * height];
    let mut rng = 0x1234_5678u32;
    for row in input.chunks_exact_mut(stride) {
        for (i, pixel) in row[..width * 4].chunks_exact_mut(4).enumerate() {
            for v in &mut *pixel {
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                *v = (rng >> 8) as f32 / 16_777_216.0;
            }
            match i % 17 {
                0 => pixel[0] = -0.1,
                1 => pixel[1] = 1.1,
                2 => pixel[2] = f32::INFINITY,
                3 => pixel[0] = f32::from_bits(0x7fc0_1234),
                _ => {}
            }
        }
    }
    let run = |src: &Profile, dst: &Profile, approximate, extra_flags| {
        let fmt = Format::RGBA_FLT;
        let plan = Plan::with_options(
            src,
            dst,
            fmt,
            fmt,
            &PlanOptions::new()
                .intent(Intent::RelativeColorimetric)
                .flags(flags::COPY_ALPHA | extra_flags)
                .approximate_float_curves(approximate),
        )
        .unwrap();
        let mut output = vec![pad; input.len()];
        plan.apply_strided(
            &mut plan.scratch(),
            bytemuck::cast_slice(&input),
            stride * 4,
            bytemuck::cast_slice_mut(&mut output),
            stride * 4,
            width,
            height,
        );
        output
    };
    let dst_bytes = Profile::srgb().save().unwrap();
    let dst = Profile::open(&dst_bytes).unwrap();
    for bytes in [
        throughput_profiles::display_p3(),
        throughput_profiles::adobe_rgb(),
    ] {
        let src = Profile::open(&bytes).unwrap();
        let exact = run(&src, &dst, false, 0);
        let sampled = run(&src, &dst, true, 0);
        let mut different = 0;
        for ((src, exact), sampled) in input
            .chunks_exact(4)
            .zip(exact.chunks_exact(4))
            .zip(sampled.chunks_exact(4))
        {
            assert_eq!(sampled[3].to_bits(), src[3].to_bits());
            let normalized = src[..3].iter().all(|x| (0.0..=1.0).contains(x));
            for (&a, &b) in sampled[..3].iter().zip(&exact[..3]) {
                if normalized {
                    assert!((a - b).abs() <= 4e-6, "sampled {a}, exact {b}");
                    different += usize::from(a.to_bits() != b.to_bits());
                } else {
                    assert_eq!(a.to_bits(), b.to_bits());
                }
            }
        }
        assert!(
            different > 0,
            "normalized matrix-shaper must take the sampled path"
        );
        let exact = run(&src, &dst, false, flags::NO_OPTIMIZE);
        let sampled = run(&src, &dst, true, flags::NO_OPTIMIZE);
        assert_eq!(
            bytemuck::cast_slice::<_, u32>(&sampled),
            bytemuck::cast_slice::<_, u32>(&exact)
        );
    }
    let clut = Profile::open(&throughput_profiles::synthetic_rgb_icc()).unwrap();
    let exact = run(&clut, &dst, false, 0);
    let sampled = run(&clut, &dst, true, 0);
    assert_eq!(
        bytemuck::cast_slice::<_, u32>(&sampled),
        bytemuck::cast_slice::<_, u32>(&exact)
    );
    for mode in [BehaviorMode::SpecExact, BehaviorMode::SpecDefault] {
        let factory = Profile::with_mode(mode);
        let src = factory.open(&throughput_profiles::display_p3()).unwrap();
        let dst = factory.open(&dst_bytes).unwrap();
        let exact = run(&src, &dst, false, 0);
        let sampled = run(&src, &dst, true, 0);
        assert_eq!(
            bytemuck::cast_slice::<_, u32>(&sampled),
            bytemuck::cast_slice::<_, u32>(&exact)
        );
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
    for precalc in [0, flags::FORCE_CLUT | flags::gridpoints(19)] {
        for cache in [0, flags::NO_CACHE] {
            check_packed_route(
                &profile,
                Format::RGB_8,
                Format::RGB_8,
                &input,
                0xa7,
                precalc | cache,
            );
        }
    }
}

#[test]
fn packed_rgb16_matches_c_on_mixed_clut_cells() {
    let mut state = 0x518f_a62eu32;
    let mut input = packed_witness(3, |i| match i {
        0..3 => 0,
        3..6 => u16::MAX,
        _ => {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state >> 16) as u16
        }
    });
    // Include corners, mixed endpoints, cell boundaries and tied fractions.
    let edge = [0, 1, 4095, 4096, 32767, 32768, 65534, 65535];
    for (i, pixel) in input[12..].as_chunks_mut::<3>().0[..192]
        .iter_mut()
        .enumerate()
    {
        *pixel = [edge[i % 8], edge[(i / 8) % 8], edge[(i / 64) * 3 + 1]];
    }
    for profile in [
        throughput_profiles::display_p3(),
        throughput_profiles::synthetic_rgb_icc(),
    ] {
        for precalc in [0, flags::gridpoints(19), flags::HIGH_RES_PRECALC] {
            for cache in [0, flags::NO_CACHE] {
                check_packed_route(
                    &profile,
                    Format::RGB_16,
                    Format::RGB_16,
                    &input,
                    0xa5a7,
                    precalc | cache,
                );
            }
        }
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
    check_float_clut_fusion(Format::RGBA_FLT, Format::RGBA_FLT, flags::COPY_ALPHA);
}

#[test]
fn rgba_float_clut_leaves_alpha_untouched() {
    check_float_clut_fusion(Format::RGBA_FLT, Format::RGBA_FLT, 0);
}

#[test]
fn rgb_float_clut_uses_normalized_samples() {
    check_float_clut_fusion(Format::RGB_FLT, Format::RGB_FLT, 0);
}

#[test]
fn cmyk_float_clut_uses_ink_percentages() {
    check_float_clut_fusion(Format::CMYK_FLT, Format::CMYK_FLT, 0);
    check_float_clut_fusion(Format::CMYK_FLT, Format::RGB_FLT, 0);
}

fn check_float_clut_fusion(input_format: Format, output_format: Format, flags: u32) {
    use lcms::{
        intrp::{InterpTable, compute_interp_params},
        lut::{StageData, StageLoc, pipeline_alloc, stage_alloc_clut16bit},
        opt::{ClutOnly, OptData},
        profile::{cms_write_tag, create_profile_placeholder},
        signatures::{profileclass, tag},
        tagtypes::TagObject,
    };

    let source_cmyk = input_format == Format::CMYK_FLT;
    let destination_cmyk = output_format == Format::CMYK_FLT;
    let n_in = if source_cmyk { 4 } else { 3 };
    let n_out = if destination_cmyk { 4 } else { 3 };
    let source_space = if source_cmyk {
        colorspace::SigCmykData
    } else {
        colorspace::SigRgbData
    };
    let destination_space = if destination_cmyk {
        colorspace::SigCmykData
    } else {
        colorspace::SigRgbData
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
    let samples: Vec<u16> = (0..1 << n_in)
        .flat_map(|corner| {
            (0..n_out).map(move |channel| {
                let high = corner & (1 << (n_in - channel - 1)) != 0;
                if high {
                    48_000 - channel as u16 * 2000
                } else {
                    7000 + channel as u16 * 1000
                }
            })
        })
        .collect();
    let stage = stage_alloc_clut16bit(None, 2, n_in, n_out, Some(&samples)).unwrap();
    let StageData::CLut(clut) = stage.data.clone() else {
        panic!("expected CLUT");
    };
    let mut pipeline = pipeline_alloc(None, n_in, n_out).unwrap();
    assert!(pipeline.insert_stage(StageLoc::AtEnd, stage));
    let mut profile = create_profile_placeholder(None);
    profile.device_class = profileclass::SigLinkClass;
    profile.color_space = source_space;
    profile.pcs = destination_space;
    assert!(cms_write_tag(
        &mut profile,
        tag::SigAToB0Tag,
        Some(TagObject::Lut16(pipeline))
    ));
    let mut input = rows;
    if source_cmyk {
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
        c::cmsSetColorSpace(handle, source_space);
        c::cmsSetPCS(handle, destination_space);
        let pipeline = c::cmsPipelineAlloc(std::ptr::null_mut(), n_in, n_out);
        assert!(!pipeline.is_null());
        let stage =
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), 2, n_in, n_out, samples.as_ptr());
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
        input_format.code(),
        output_format.code(),
        flags,
    )
    .unwrap();
    let mut expected = [f32::from_bits(0x3f37_19a1); OUT_WORDS * HEIGHT as usize];
    let mut actual = expected;
    unsafe {
        let ct = c::cmsCreateTransform(
            c_profile,
            input_format.code(),
            std::ptr::null_mut(),
            output_format.code(),
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
    if output_format == Format::RGBA_FLT {
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
        "{input_format:?} -> {output_format:?} stock flags={flags:#x}"
    );

    // The stock optimizer does not resample float formats. Attach the
    // same CLUT as an optimized table to exercise the fused worker.
    let lut = transform.plan.lut.as_mut().unwrap();
    assert!(lut.opt_data.is_none());
    assert_eq!(lut.stage_count(), 1);
    lut.opt_data = Some(OptData::ClutOnly(Box::new(ClutOnly {
        clut: clut.as_ref().clone(),
        params16: compute_interp_params(
            None,
            2,
            n_in,
            n_out,
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
        "{input_format:?} -> {output_format:?} fused flags={flags:#x}"
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
