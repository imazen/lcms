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
        }
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
    let mox = T::mox(
        &m_src,
        &m_dst,
        route,
        TransformOptions {
            rendering_intent: RenderingIntent::RelativeColorimetric,
            allow_use_cicp_transfer: false,
            ..Default::default()
        },
    );
    if std::env::var_os("LCMS_BENCH_COMPARE_OUTPUTS").is_some() {
        let mut oracle = vec![T::default(); out_row];
        let mut actual = oracle.clone();
        let c = CTransform::new(src_icc, dst_icc, route.src_format, route.dst_format, alpha);
        c.apply(&lcms_input[..in_row], &mut oracle, width);
        mox.transform(&input[..in_row], &mut actual).unwrap();
        let size = size_of::<T>();
        let different = bytemuck::cast_slice::<T, u8>(&oracle)
            .chunks_exact(size)
            .zip(bytemuck::cast_slice::<T, u8>(&actual).chunks_exact(size))
            .filter(|(a, b)| a != b)
            .count();
        let max_delta = oracle
            .into_iter()
            .zip(actual)
            .map(|(a, b)| (a.into() - b.into()).abs())
            .fold(0.0, f64::max);
        eprintln!(
            "{} moxcms vs C: {different}/{out_row} samples differ, max abs delta {max_delta:.9}",
            route.label,
        );
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
        let mut output = vec![T::default(); out_row * rows];
        g.bench("moxcms 0.9.2", move |b| {
            b.iter(|| {
                for (src, dst) in input
                    .chunks_exact(in_row)
                    .zip(output.chunks_exact_mut(out_row))
                {
                    mox.transform(src, dst).unwrap();
                }
                black_box(&output);
            });
        });
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
    bench::<f32>(suite, Route::rgb("p3-rgbf32", Format::RGB_FLT), &p3, &dst);
    bench::<f32>(suite, Route::rgb("p3-rgbaf32", Format::RGBA_FLT), &p3, &dst);
    bench_with_samples(
        suite,
        Route::rgb("p3-noise-rgbf32", Format::RGB_FLT),
        &p3,
        &dst,
        noise_f32,
    );
    bench::<u8>(suite, Route::rgb("adobe-rgb8", Format::RGB_8), &adobe, &dst);
    bench::<f32>(
        suite,
        Route::rgb("adobe-rgbf32", Format::RGB_FLT),
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
