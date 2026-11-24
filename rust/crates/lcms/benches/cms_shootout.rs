//! Throughput of packed, row-by-row ICC conversion with reusable output.

#[path = "../tests/support/throughput_profiles.rs"]
mod profiles;
use lcms::api::{Format, Intent, Plan, PlanOptions, Profile, flags};
use lcms_cref as c;
use moxcms::{ColorProfile, Layout, RenderingIntent, TransformOptions};
use profiles::{adobe_rgb, display_p3, synthetic_cmyk_icc, synthetic_rgb_icc};
use std::ffi::c_void;
use std::path::Path;
use std::sync::Arc;
use zenbench::prelude::*;

const WIDTH: usize = 3840;
const HEIGHT: usize = 2160;
const CMYK_WIDTH: usize = 4096;

fn matrix_profile(name: &str, build: impl FnOnce() -> Vec<u8>) -> Vec<u8> {
    match std::env::var_os("LCMS_BENCH_PROFILE_DIR") {
        Some(dir) => std::fs::read(Path::new(&dir).join(name)).expect("benchmark ICC fixture"),
        None => build(),
    }
}

struct CTransform(*mut c_void);
// Zenbench takes a Send closure. Each transform is owned by one benchmark arm.
unsafe impl Send for CTransform {}

impl CTransform {
    fn new(src: &[u8], dst: &[u8], src_format: Format, dst_format: Format, flags: u32) -> Self {
        unsafe {
            let from = c::cmsOpenProfileFromMem(src.as_ptr().cast(), src.len() as u32);
            let to = c::cmsOpenProfileFromMem(dst.as_ptr().cast(), dst.len() as u32);
            assert!(!from.is_null() && !to.is_null());
            let handle =
                c::cmsCreateTransform(from, src_format.code(), to, dst_format.code(), 1, flags);
            c::cmsCloseProfile(from);
            c::cmsCloseProfile(to);
            assert!(!handle.is_null());
            Self(handle)
        }
    }

    fn apply<T>(&self, src: &[T], dst: &mut [T], width: usize) {
        unsafe {
            c::cmsDoTransform(
                self.0,
                src.as_ptr().cast(),
                dst.as_mut_ptr().cast(),
                width as u32,
            )
        }
    }
}

impl Drop for CTransform {
    fn drop(&mut self) {
        unsafe { c::cmsDeleteTransform(self.0) }
    }
}

#[derive(Clone, Copy)]
struct Route {
    label: &'static str,
    src_format: Format,
    dst_format: Format,
    src_ch: usize,
    dst_ch: usize,
    src_layout: Layout,
    dst_layout: Layout,
    float_matrix: bool,
}

impl Route {
    fn rgb(label: &'static str, format: Format) -> Self {
        let (channels, layout) = if [Format::RGBA_8, Format::RGBA_FLT].contains(&format) {
            (4, Layout::Rgba)
        } else {
            (3, Layout::Rgb)
        };
        Self {
            label,
            src_format: format,
            dst_format: format,
            src_ch: channels,
            dst_ch: channels,
            src_layout: layout,
            dst_layout: layout,
            float_matrix: false,
        }
    }

    fn cmyk(label: &'static str, src_format: Format, dst_format: Format) -> Self {
        Self {
            label,
            src_format,
            dst_format,
            src_ch: 4,
            dst_ch: 3,
            src_layout: Layout::Rgba,
            dst_layout: Layout::Rgb,
            float_matrix: false,
        }
    }

    fn float_matrix(mut self) -> Self {
        self.float_matrix = true;
        self
    }
}

trait Sample: bytemuck::Pod + Default + Into<f64> + Send + Sync + 'static {
    fn generate(i: usize) -> Self;

    fn cmyk_percent(self) -> Self {
        self
    }

    fn mox(
        src: &ColorProfile,
        dst: &ColorProfile,
        route: Route,
        options: TransformOptions,
    ) -> Arc<dyn moxcms::TransformExecutor<Self> + Send + Sync>;
}

impl Sample for u8 {
    fn generate(i: usize) -> Self {
        ((i * 37 + i / 7 * 13) % 251) as u8
    }
    fn mox(
        src: &ColorProfile,
        dst: &ColorProfile,
        r: Route,
        opts: TransformOptions,
    ) -> Arc<dyn moxcms::TransformExecutor<Self> + Send + Sync> {
        src.create_transform_8bit(r.src_layout, dst, r.dst_layout, opts)
            .unwrap()
    }
}

impl Sample for u16 {
    fn generate(i: usize) -> Self {
        (i * 9973 + 12345) as u16
    }
    fn mox(
        src: &ColorProfile,
        dst: &ColorProfile,
        r: Route,
        opts: TransformOptions,
    ) -> Arc<dyn moxcms::TransformExecutor<Self> + Send + Sync> {
        src.create_transform_16bit(r.src_layout, dst, r.dst_layout, opts)
            .unwrap()
    }
}

impl Sample for f32 {
    fn generate(i: usize) -> Self {
        ((i * 37 + 17) % 1009) as f32 / 1008.0
    }
    fn cmyk_percent(self) -> Self {
        self * 100.0
    }
    fn mox(
        src: &ColorProfile,
        dst: &ColorProfile,
        r: Route,
        opts: TransformOptions,
    ) -> Arc<dyn moxcms::TransformExecutor<Self> + Send + Sync> {
        src.create_transform_f32(r.src_layout, dst, r.dst_layout, opts)
            .unwrap()
    }
}

fn bench<T: Sample>(suite: &mut Suite, route: Route, src_icc: &[u8], dst_icc: &[u8]) {
    bench_with_samples(suite, route, src_icc, dst_icc, T::generate);
}

fn compare_outputs<T: Sample>(route: Route, engine: &str, exact: &[T], actual: &[T]) {
    let size = size_of::<T>();
    let different = bytemuck::cast_slice::<T, u8>(exact)
        .chunks_exact(size)
        .zip(bytemuck::cast_slice::<T, u8>(actual).chunks_exact(size))
        .filter(|(a, b)| a != b)
        .count();
    let quantize = |x: f64, max: f64| (x.clamp(0.0, 1.0) * max).round() as u16;
    let mut q8 = 0;
    let mut q16 = 0;
    let mut max_delta = 0.0f64;
    let mut max_in_gamut = 0.0f64;
    let mut out_of_gamut = 0;
    for (&a, &b) in exact.iter().zip(actual) {
        let (a, b) = (a.into(), b.into());
        let delta = (a - b).abs();
        max_delta = max_delta.max(delta);
        if route.dst_format.is_float() {
            if (0.0..=1.0).contains(&a) {
                max_in_gamut = max_in_gamut.max(delta);
            } else {
                out_of_gamut += 1;
            }
            q8 += usize::from(quantize(a, 255.0) != quantize(b, 255.0));
            q16 += usize::from(quantize(a, 65535.0) != quantize(b, 65535.0));
        }
    }
    eprint!(
        "{} {engine}: {different}/{} samples differ, max abs delta {max_delta:.9}",
        route.label,
        exact.len()
    );
    if route.dst_format.is_float() {
        eprint!(
            ", max in-gamut delta {max_in_gamut:.9}, {out_of_gamut} out-of-range reference samples, u8 {q8}, u16 {q16} different after rounding"
        );
    }
    eprintln!();
}

fn bench_with_samples<T: Sample>(
    suite: &mut Suite,
    route: Route,
    src_icc: &[u8],
    dst_icc: &[u8],
    generate: impl Fn(usize) -> T,
) {
    let cmyk = route.src_ch > route.dst_ch;
    let width = if cmyk { CMYK_WIDTH } else { WIDTH };
    let rows = if cmyk { 1 } else { HEIGHT };
    let pixels = width * rows;
    let in_row = width * route.src_ch;
    let out_row = width * route.dst_ch;
    let input: Arc<[T]> = (0..pixels * route.src_ch).map(generate).collect();
    let lcms_input: Arc<[T]> = if cmyk {
        input.iter().copied().map(T::cmyk_percent).collect()
    } else {
        Arc::clone(&input)
    };
    let src = Profile::open(src_icc).unwrap();
    let dst = Profile::open(dst_icc).unwrap();
    let m_src = ColorProfile::new_from_slice(src_icc).unwrap();
    let m_dst = ColorProfile::new_from_slice(dst_icc).unwrap();
    let alpha = if route.dst_ch == 4 {
        flags::COPY_ALPHA
    } else {
        0
    };
    let mox_options = TransformOptions {
        rendering_intent: RenderingIntent::RelativeColorimetric,
        allow_use_cicp_transfer: false,
        ..Default::default()
    };
    let mox = T::mox(&m_src, &m_dst, route, mox_options);
    let mox_extended = route.float_matrix.then(|| {
        T::mox(
            &m_src,
            &m_dst,
            route,
            TransformOptions {
                allow_extended_range_rgb_xyz: true,
                ..mox_options
            },
        )
    });
    let options = PlanOptions::new()
        .intent(Intent::RelativeColorimetric)
        .flags(alpha);
    let sampled = route.float_matrix.then(|| {
        Plan::with_options(
            &src,
            &dst,
            route.src_format,
            route.dst_format,
            &options.approximate_float_curves(true),
        )
        .unwrap()
    });
    if std::env::var_os("LCMS_BENCH_COMPARE_OUTPUTS").is_some() {
        let mut oracle = vec![T::default(); out_row];
        let mut actual = oracle.clone();
        let c = CTransform::new(src_icc, dst_icc, route.src_format, route.dst_format, alpha);
        c.apply(&lcms_input[..in_row], &mut oracle, width);
        let exact =
            Plan::with_options(&src, &dst, route.src_format, route.dst_format, &options).unwrap();
        exact.apply_pixels(&mut exact.scratch(), &lcms_input[..in_row], &mut actual);
        compare_outputs(route, "Rust lcms exact vs C", &oracle, &actual);
        mox.transform(&input[..in_row], &mut actual).unwrap();
        compare_outputs(route, "moxcms default vs C", &oracle, &actual);
        if let Some(mox) = &mox_extended {
            mox.transform(&input[..in_row], &mut actual).unwrap();
            compare_outputs(route, "moxcms extended-range vs C", &oracle, &actual);
        }
        if let Some(plan) = &sampled {
            plan.apply_pixels(&mut plan.scratch(), &lcms_input[..in_row], &mut actual);
            compare_outputs(route, "Rust lcms sampled curves vs C", &oracle, &actual);
        }
    }
    suite.compare(route.label, |g| {
        g.throughput(Throughput::Elements(pixels as u64));
        let rounds = if cmyk { 20 } else { 6 };
        g.config()
            .auto_rounds(false)
            .min_rounds(rounds)
            .max_rounds(rounds);
        let modes = [
            ("default", 0),
            ("no-cache", flags::NO_CACHE),
            (
                "no-cache + BPC",
                flags::NO_CACHE | flags::BLACK_POINT_COMPENSATION,
            ),
        ];
        for &(suffix, mode) in &modes[..if cmyk { 3 } else { 2 }] {
            // Both LCMS engines already turn off caching for float samples.
            if route.src_format.is_float() && mode == flags::NO_CACHE {
                continue;
            }
            let options = PlanOptions::new()
                .intent(Intent::RelativeColorimetric)
                .flags(mode | alpha);
            let plan = Plan::with_options(&src, &dst, route.src_format, route.dst_format, &options)
                .unwrap();
            let mut scratch = plan.scratch();
            let rust_input = Arc::clone(&lcms_input);
            let mut output = vec![T::default(); out_row * rows];
            g.bench(format!("Rust lcms ({suffix})"), move |b| {
                b.iter(|| {
                    for (src, dst) in rust_input
                        .chunks_exact(in_row)
                        .zip(output.chunks_exact_mut(out_row))
                    {
                        plan.apply_pixels(&mut scratch, src, dst);
                    }
                    black_box(&output);
                });
            });
            let c = CTransform::new(
                src_icc,
                dst_icc,
                route.src_format,
                route.dst_format,
                mode | alpha,
            );
            let lcms_input = Arc::clone(&lcms_input);
            let mut output = vec![T::default(); out_row * rows];
            g.bench(format!("C Little CMS 2.20 ({suffix})"), move |b| {
                b.iter(|| {
                    for (src, dst) in lcms_input
                        .chunks_exact(in_row)
                        .zip(output.chunks_exact_mut(out_row))
                    {
                        c.apply(src, dst, width);
                    }
                    black_box(&output);
                });
            });
        }
        if let Some(plan) = sampled {
            let mut scratch = plan.scratch();
            let mut output = vec![T::default(); out_row * rows];
            g.bench("Rust lcms (sampled float curves)", move |b| {
                b.iter(|| {
                    for (src, dst) in lcms_input
                        .chunks_exact(in_row)
                        .zip(output.chunks_exact_mut(out_row))
                    {
                        plan.apply_pixels(&mut scratch, src, dst);
                    }
                    black_box(&output);
                });
            });
        }
        for (name, transform) in [
            ("moxcms 0.9.2", Some(mox)),
            ("moxcms 0.9.2 (extended range)", mox_extended),
        ] {
            if let Some(transform) = transform {
                let input = input.clone();
                let mut output = vec![T::default(); out_row * rows];
                g.bench(name, move |b| {
                    b.iter(|| {
                        for (src, dst) in input
                            .chunks_exact(in_row)
                            .zip(output.chunks_exact_mut(out_row))
                        {
                            transform.transform(src, dst).unwrap();
                        }
                        black_box(&output);
                    });
                });
            }
        }
    });
}

fn noise_f32(i: usize) -> f32 {
    let mut x = (i as u32).wrapping_add(0x9e37_79b9);
    x = (x ^ (x >> 16)).wrapping_mul(0x21f0_aaad);
    x = (x ^ (x >> 15)).wrapping_mul(0x735a_2d97);
    ((x ^ (x >> 15)) >> 8) as f32 / 16_777_216.0
}

zenbench::main!(|suite| {
    let p3 = matrix_profile("DisplayP3Compat-v4.icc", display_p3);
    let adobe = matrix_profile("AdobeCompat-v2.icc", adobe_rgb);
    let dst = Profile::srgb().save().unwrap();
    let rgb = synthetic_rgb_icc();
    let cmyk = synthetic_cmyk_icc();
    bench::<u8>(suite, Route::rgb("p3-rgb8", Format::RGB_8), &p3, &dst);
    bench::<u8>(suite, Route::rgb("p3-rgba8", Format::RGBA_8), &p3, &dst);
    bench::<u16>(suite, Route::rgb("p3-rgb16", Format::RGB_16), &p3, &dst);
    bench::<f32>(
        suite,
        Route::rgb("p3-rgbf32", Format::RGB_FLT).float_matrix(),
        &p3,
        &dst,
    );
    bench::<f32>(
        suite,
        Route::rgb("p3-rgbaf32", Format::RGBA_FLT).float_matrix(),
        &p3,
        &dst,
    );
    bench_with_samples(
        suite,
        Route::rgb("p3-noise-rgbf32", Format::RGB_FLT).float_matrix(),
        &p3,
        &dst,
        noise_f32,
    );
    bench::<u8>(suite, Route::rgb("adobe-rgb8", Format::RGB_8), &adobe, &dst);
    bench::<f32>(
        suite,
        Route::rgb("adobe-rgbf32", Format::RGB_FLT).float_matrix(),
        &adobe,
        &dst,
    );
    bench::<u8>(suite, Route::rgb("lut-rgb8", Format::RGB_8), &rgb, &dst);
    bench::<u16>(suite, Route::rgb("lut-rgb16", Format::RGB_16), &rgb, &dst);
    bench::<f32>(suite, Route::rgb("lut-rgbf32", Format::RGB_FLT), &rgb, &dst);
    bench::<u8>(
        suite,
        Route::cmyk("lut-cmyk8-rgb8", Format::CMYK_8, Format::RGB_8),
        &cmyk,
        &dst,
    );
    bench::<u16>(
        suite,
        Route::cmyk("lut-cmyk16-rgb16", Format::CMYK_16, Format::RGB_16),
        &cmyk,
        &dst,
    );
    bench::<f32>(
        suite,
        Route::cmyk("lut-cmykf32-rgbf32", Format::CMYK_FLT, Format::RGB_FLT),
        &cmyk,
        &dst,
    );
});
